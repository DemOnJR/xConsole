//! MCP over Streamable HTTP, for an agent that is not a child process.
//!
//! The stdio transport works because the CLI runs on this machine and xConsole can be
//! its subprocess. Claude Code running *on a VPS* can be neither — it cannot spawn an
//! executable that lives on the user's laptop. Claude Code's remaining transports are
//! HTTP, SSE and WebSocket, all of which need a URL the remote box can reach.
//!
//! So: bind a loopback listener here, and let [`crate::ssh::tunnel::open_reverse_forward`]
//! give the VPS a `127.0.0.1` port that tunnels back to it over the SSH session xConsole
//! already holds open.
//!
//! # Why this does not undo "no inbound port"
//!
//! xConsole's whole premise is that nothing on the internet can reach the machine. That
//! still holds:
//!
//! - The listener binds `127.0.0.1`, so it is unreachable from this machine's network.
//! - The route in is an SSH channel on a connection *this* app dialled out. There is no
//!   listening socket on any public interface, at either end — the remote side is bound
//!   to the VPS's own loopback too.
//! - Every request must carry a bearer token minted per run and never written to disk.
//! - The listener lives exactly as long as the run and is dropped with it.
//!
//! There is no HTTP framework in this tree, and the project's dependency policy makes
//! adding one a deliberate act rather than a convenience. The subset needed here is
//! small enough to write directly: one method, one path, a JSON body in, a JSON body
//! out.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::server::{dispatch_message, McpSession};

/// Refuse a body larger than this. A tool call is small; anything at this size is a bug
/// or an attempt to exhaust memory through the tunnel.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// A running loopback MCP endpoint. Dropping it stops serving.
pub(crate) struct McpHttpServer {
    pub port: u16,
    pub token: String,
    accept_loop: tokio::task::JoinHandle<()>,
}

impl Drop for McpHttpServer {
    fn drop(&mut self) {
        self.accept_loop.abort();
    }
}

/// Start the endpoint on an ephemeral loopback port.
///
/// The token is generated here rather than taken as an argument so there is no way to
/// start one without authentication.
pub(crate) async fn serve(mut session: McpSession) -> Result<McpHttpServer, String> {
    let token = uuid::Uuid::new_v4().to_string();
    // The session is what actually checks the token on each request; minting one here
    // and forgetting to install it would 401 every call.
    session.set_token(token.clone());
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not open the local MCP port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();

    let session = Arc::new(session);
    let accept_loop = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let session = session.clone();
            // One task per connection: a client that opens a socket and then goes quiet
            // must not stop the next request being served.
            tokio::spawn(async move {
                let _ = serve_connection(stream, session).await;
            });
        }
    });

    Ok(McpHttpServer {
        port,
        token,
        accept_loop,
    })
}

async fn serve_connection(
    mut stream: tokio::net::TcpStream,
    session: Arc<McpSession>,
) -> Result<(), String> {
    // Keep-alive: Claude Code reuses one connection for a whole session's tool calls.
    loop {
        let Some(request) = read_request(&mut stream).await? else {
            return Ok(()); // client hung up between requests
        };

        let response = match handle(&request, &session).await {
            Ok(body) => http_response(200, "application/json", &body),
            Err((code, message)) => http_response(
                code,
                "application/json",
                &json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32600, "message": message },
                })
                .to_string(),
            ),
        };

        stream
            .write_all(response.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stream.flush().await.map_err(|e| e.to_string())?;
    }
}

struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: String,
}

/// Read one HTTP/1.1 request.
///
/// Only what this endpoint actually receives is supported: a request line, headers, and
/// a `Content-Length` body. Chunked encoding is refused rather than mis-parsed, which is
/// the failure mode that turns a protocol gap into a security bug.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<Option<Request>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    // Headers first: read until the blank line that ends them.
    let header_end = loop {
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return Err("MCP request headers too large".into());
        }
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0usize;
    let mut authorization = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => content_length = value.parse().unwrap_or(0),
            "authorization" => authorization = Some(value.to_string()),
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => {
                return Err("chunked request bodies are not supported".into());
            }
            _ => {}
        }
    }

    if content_length > MAX_BODY_BYTES {
        return Err("MCP request body too large".into());
    }

    // Whatever arrived with the headers is the start of the body.
    let mut body = buf.split_off(header_end + 4);
    while body.len() < content_length {
        let n = stream.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);

    Ok(Some(Request {
        method,
        path,
        authorization,
        body: String::from_utf8_lossy(&body).to_string(),
    }))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn handle(request: &Request, session: &Arc<McpSession>) -> Result<String, (u16, String)> {
    if !request.path.starts_with("/mcp") {
        return Err((404, "not found".into()));
    }
    if request.method != "POST" {
        // GET opens a server-initiated SSE stream in the full spec. Nothing here sends
        // unsolicited messages, so saying no is honest and keeps the surface at one verb.
        return Err((405, "only POST is supported".into()));
    }

    // Checked before parsing: an unauthenticated caller should not reach the JSON parser.
    let presented = request
        .authorization
        .as_deref()
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim();
    if !session_token_matches(session, presented) {
        return Err((401, "unauthorized".into()));
    }

    let Ok(msg) = serde_json::from_str::<Value>(&request.body) else {
        return Err((400, "request body is not JSON".into()));
    };

    // A notification carries no id and expects no reply.
    let Some(id) = msg.get("id").cloned() else {
        return Ok(String::new());
    };
    let method = msg
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    Ok(dispatch_message(session, &method, &id, &msg).await)
}

/// Compare in constant time: a token checked with `==` leaks its prefix to anything that
/// can time the response, and this one guards SSH access to the user's servers.
fn session_token_matches(session: &Arc<McpSession>, presented: &str) -> bool {
    let expected = session.token_for_auth();
    if expected.is_empty() || expected.len() != presented.len() {
        return false;
    }
    expected
        .bytes()
        .zip(presented.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn http_response(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: keep-alive\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_is_found_after_the_blank_line() {
        assert_eq!(find_header_end(b"POST /mcp\r\n\r\nbody"), Some(9));
        assert_eq!(find_header_end(b"POST /mcp\r\nno end yet"), None);
    }

    #[test]
    fn the_response_declares_the_byte_length_not_the_char_length() {
        // A tool result carrying non-ASCII (a path, a log line) would truncate at the
        // client if Content-Length counted chars.
        let body = "{\"result\":\"café\"}";
        let out = http_response(200, "application/json", body);
        assert!(out.contains(&format!("Content-Length: {}", body.len())));
        assert!(out.len() > body.len());
    }
}

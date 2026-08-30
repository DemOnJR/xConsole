//! Local port forwarding over an existing SSH connection.
//!
//! This is the piece that lets a normal database client talk to a server that isn't
//! exposed to the internet. A listener is opened on `127.0.0.1:0`; every inbound
//! connection gets its own `direct-tcpip` channel on the SSH session, and bytes are
//! pumped both ways. A client then connects to the local port and, as far as it is
//! concerned, is talking straight to the remote service.
//!
//! Two reasons to do it this way rather than teaching a MySQL crate to speak over an
//! SSH channel:
//!
//! - Every client library already knows how to connect to a TCP socket, so no driver
//!   needs a custom transport. That also means a container's database reachable only on
//!   the Docker bridge (`172.17.0.x:3306`) works unchanged — the SSH server does the
//!   routing, because the channel's destination is resolved on its side.
//! - It reuses the connection the user already authenticated, so no second handshake and
//!   no second credential prompt.
//!
//! The listener binds to loopback only, so nothing outside this machine can reach it.
//!
//! Not yet called from anywhere: the database client currently runs queries through the
//! host's own `mysql` client over an exec channel, which needs no forward. This exists
//! for the switch to a native driver (a driver needs a socket, and this is the socket),
//! and is kept because it is the load-bearing half of that change and is easier to
//! review on its own than bundled into it.
#![allow(dead_code)]

use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::client::Handler;

/// A live forward. Dropping it stops accepting new connections.
pub struct Tunnel {
    /// Loopback port to point a client at.
    pub local_port: u16,
    /// Where it comes out, for diagnostics.
    pub remote: String,
    accept_loop: JoinHandle<()>,
}

impl Tunnel {
    /// Where a client should connect.
    pub fn local_addr(&self) -> String {
        format!("127.0.0.1:{}", self.local_port)
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        // Stops the accept loop; connections already being pumped finish on their own
        // tasks rather than being cut mid-query.
        self.accept_loop.abort();
    }
}

/// Forward a loopback port to `remote_host:remote_port` through `ssh`.
///
/// `remote_host` is resolved by the SSH server, not locally — so `127.0.0.1` means the
/// server's own loopback (where a database usually listens), and a container IP works
/// without publishing a port.
pub async fn open(
    ssh: Arc<russh::client::Handle<Handler>>,
    remote_host: &str,
    remote_port: u16,
) -> Result<Tunnel, String> {
    // Port 0 lets the OS pick a free one, avoiding a race against anything else on the
    // machine that might have taken a port we guessed.
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not open a local port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("could not read the local port: {e}"))?
        .port();

    let remote = format!("{remote_host}:{remote_port}");
    let host = remote_host.to_string();

    let accept_loop = tokio::spawn(async move {
        loop {
            let (mut inbound, peer) = match listener.accept().await {
                Ok(pair) => pair,
                // The listener is gone (tunnel dropped) — nothing left to do.
                Err(_) => break,
            };

            let ssh = ssh.clone();
            let host = host.clone();
            tokio::spawn(async move {
                // russh wants the originator for the protocol message; the real peer
                // address is the honest thing to report.
                let channel = match ssh
                    .channel_open_direct_tcpip(
                        host.clone(),
                        remote_port as u32,
                        peer.ip().to_string(),
                        peer.port() as u32,
                    )
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("xconsole: tunnel to {host}:{remote_port} refused: {e}");
                        return;
                    }
                };

                let mut outbound = channel.into_stream();
                // Ignore the outcome: either side closing is the normal way a forwarded
                // connection ends, and a client disconnecting is not an error.
                let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
            });
        }
    });

    Ok(Tunnel { local_port, remote, accept_loop })
}

/// A reverse SSH forward (binds on remote VPS loopback, tunnels back to local endpoint).
pub struct ReverseTunnel {
    pub remote_bind_addr: String,
    pub remote_port: u32,
}

/// Request the remote SSH server to listen on `127.0.0.1:remote_port` and forward connections
/// back through this SSH session. Zero public IPs, loopback only.
pub async fn open_reverse_forward(
    ssh: &russh::client::Handle<Handler>,
    remote_port: u32,
) -> Result<ReverseTunnel, String> {
    let bound_port = ssh
        .tcpip_forward("127.0.0.1", remote_port)
        .await
        .map_err(|e| format!("could not establish reverse SSH port forward: {e}"))?;

    // Asking for port 0 means "server, pick one", and the answer comes back in the
    // reply. A zero here means we never learned it — and the caller would go on to
    // build `http://127.0.0.1:0/mcp`, which nothing can connect to. The agent then runs
    // with none of xConsole's tools and explains to the user that it cannot reach their
    // servers, which looks like the agent being unhelpful rather than a dead tunnel.
    let port = if bound_port == 0 { remote_port } else { bound_port };
    if port == 0 {
        return Err(
            "the SSH server granted a reverse forward but did not say which port it \
             bound, so there is no address to hand the agent. Set a fixed port instead \
             of asking the server to choose."
                .into(),
        );
    }

    Ok(ReverseTunnel {
        remote_bind_addr: "127.0.0.1".into(),
        remote_port: port,
    })
}


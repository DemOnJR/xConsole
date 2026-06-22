//! Concrete `Provider` implementations.

pub mod anthropic;
pub mod cli;
pub mod ollama;
pub mod openai_compat;

/// Incremental decoder for Server-Sent-Events `data:` lines.
///
/// Both the Anthropic and OpenAI streaming APIs frame each event as one or more
/// `data: <payload>` lines terminated by a blank line. We only need the payload,
/// so this yields each `data:` payload as it completes across chunk boundaries.
#[derive(Default)]
pub struct SseBuffer {
    buf: String,
}

impl SseBuffer {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    /// Feed a chunk; return any completed `data:` payloads.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        // Process complete lines; keep the trailing partial line buffered.
        while let Some(idx) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=idx).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = line.strip_prefix("data:") {
                out.push(rest.trim_start().to_string());
            }
        }
        out
    }
}

/// Join a base URL and a path without doubling slashes.
pub fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

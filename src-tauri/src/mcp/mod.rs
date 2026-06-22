//! MCP stdio bridge so Cursor Agent CLI can SSH via xConsole.

mod server;
mod workspace;

pub use workspace::prepare_cursor_workspace;

use server::run_stdio_server;

/// Entry point for `xconsole.exe --xconsole-mcp-stdio`.
pub fn run_stdio() {
    if let Err(e) = run_stdio_server() {
        eprintln!("xconsole mcp error: {e}");
        std::process::exit(1);
    }
}

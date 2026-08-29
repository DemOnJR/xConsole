//! MCP stdio bridge so Cursor Agent CLI can SSH via xConsole.

mod server;
mod workspace;

pub use workspace::{prepare_agent_workspace, prepare_cursor_workspace};

/// Entry point for `xconsole.exe --xconsole-mcp-stdio`.
pub fn run_stdio() {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("xconsole mcp runtime init error: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = rt.block_on(server::run_stdio_server()) {
        eprintln!("xconsole mcp error: {e}");
        std::process::exit(1);
    }
}

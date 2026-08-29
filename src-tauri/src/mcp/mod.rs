//! MCP bridges so an external agent CLI can SSH via xConsole.
//!
//! Two transports, for two places the agent can run. [`server`] speaks JSON-RPC over
//! stdio, for a CLI running on this machine as a child process. [`http`] serves the same
//! dispatcher over a loopback HTTP port, for a CLI running on a VPS that reaches back
//! through an SSH reverse forward.

pub(crate) mod http;
mod server;
mod workspace;

pub use workspace::{prepare_agent_workspace, prepare_cursor_workspace};

/// An MCP session for a tunnelled agent, scoped to one turn's targets and safety mode.
///
/// The token is minted by [`http::serve`], not here — a session with no token would
/// serve SSH access to anything that reached the port.
pub(crate) fn server_session_for_bridge(
    db: crate::storage::Db,
    data_dir: &std::path::Path,
    targets: Vec<String>,
    safety: String,
    workspace_id: String,
) -> server::McpSession {
    server::McpSession::in_process(
        db,
        crate::ai::AgentHome::new(data_dir.join("agent")),
        data_dir,
        targets,
        safety,
        workspace_id,
        String::new(),
    )
}

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

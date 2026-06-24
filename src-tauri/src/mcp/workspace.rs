//! Write a Cursor workspace with `.cursor/mcp.json` pointing at this binary.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

/// Create `{data_dir}/cursor-workspaces/{session_id}/.cursor/mcp.json` and return the workspace root.
pub fn prepare_cursor_workspace(
    data_dir: &Path,
    session_id: &str,
    targets: &[String],
    safety: &str,
    workspace_id: &str,
) -> Result<PathBuf, String> {
    let root = data_dir.join("cursor-workspaces").join(session_id);
    let cursor_dir = root.join(".cursor");
    fs::create_dir_all(&cursor_dir).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let agent_home = data_dir.join("agent");

    // serde_json handles all string escaping (Windows backslashes, quotes, etc.)
    // — never hand-build JSON via format!.
    let mcp = json!({
        "mcpServers": {
            "xconsole": {
                "command": exe.to_string_lossy(),
                "args": ["--xconsole-mcp-stdio"],
                "env": {
                    "XCONSOLE_DATA_DIR": data_dir.to_string_lossy(),
                    "XCONSOLE_AGENT_HOME": agent_home.to_string_lossy(),
                    "XCONSOLE_TARGETS": targets.join(","),
                    "XCONSOLE_SAFETY": safety,
                    "XCONSOLE_WORKSPACE_ID": workspace_id,
                }
            }
        }
    });
    let pretty = serde_json::to_string_pretty(&mcp).map_err(|e| e.to_string())?;

    fs::write(cursor_dir.join("mcp.json"), pretty).map_err(|e| e.to_string())?;
    Ok(root)
}

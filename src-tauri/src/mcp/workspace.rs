//! Write a Cursor workspace with `.cursor/mcp.json` pointing at this binary.

use std::fs;
use std::path::{Path, PathBuf};

/// Create `{data_dir}/cursor-workspaces/{session_id}/.cursor/mcp.json` and return the workspace root.
pub fn prepare_cursor_workspace(
    data_dir: &Path,
    session_id: &str,
    targets: &[String],
    safety: &str,
) -> Result<PathBuf, String> {
    let root = data_dir
        .join("cursor-workspaces")
        .join(session_id);
    let cursor_dir = root.join(".cursor");
    fs::create_dir_all(&cursor_dir).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().replace('\\', "\\\\");

    let data_str = data_dir.to_string_lossy().replace('\\', "\\\\");
    let agent_home = data_dir.join("agent");
    let agent_str = agent_home.to_string_lossy().replace('\\', "\\\\");
    let targets_str = targets.join(",");

    let mcp_json = format!(
        r#"{{
  "mcpServers": {{
    "xconsole": {{
      "command": "{exe_str}",
      "args": ["--xconsole-mcp-stdio"],
      "env": {{
        "XCONSOLE_DATA_DIR": "{data_str}",
        "XCONSOLE_AGENT_HOME": "{agent_str}",
        "XCONSOLE_TARGETS": "{targets_str}",
        "XCONSOLE_SAFETY": "{safety}"
      }}
    }}
  }}
}}"#
    );

    fs::write(cursor_dir.join("mcp.json"), mcp_json).map_err(|e| e.to_string())?;
    Ok(root)
}

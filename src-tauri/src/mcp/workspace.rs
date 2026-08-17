//! Write an agent workspace with MCP configuration pointing at this binary.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

/// Create `{data_dir}/workspaces/{session_id}/` (with `.agents/mcp_config.json`, `.cursor/mcp.json`, `mcp_config.json`, `AGENTS.md`)
/// and return the workspace root.
pub fn prepare_agent_workspace(
    data_dir: &Path,
    session_id: &str,
    targets: &[String],
    safety: &str,
    workspace_id: &str,
) -> Result<PathBuf, String> {
    let root = data_dir.join("workspaces").join(session_id);
    let cursor_dir = root.join(".cursor");
    let agents_dir = root.join(".agents");
    let claude_dir = root.join(".claude");

    fs::create_dir_all(&cursor_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&agents_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&claude_dir).map_err(|e| e.to_string())?;

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

    // 1. Cursor MCP config
    fs::write(cursor_dir.join("mcp.json"), &pretty).map_err(|e| e.to_string())?;

    // 2. Antigravity workspace customizations (.agents/mcp_config.json)
    fs::write(agents_dir.join("mcp_config.json"), &pretty).map_err(|e| e.to_string())?;

    // 3. Claude Code MCP config (.claude/mcp.json)
    fs::write(claude_dir.join("mcp.json"), &pretty).map_err(|e| e.to_string())?;

    // 4. Root MCP config (mcp_config.json)
    fs::write(root.join("mcp_config.json"), &pretty).map_err(|e| e.to_string())?;

    // 5. Agent Instructions (AGENTS.md & GEMINI.md)
    let instructions = r#"# Remote VPS Environment

You are working directly on the user's remote Linux VPS server(s).
All MCP tools (`run_command`, `read_file`, `write_file`, `list_vps_targets`) execute directly against the active VPS target.

## OPERATIONAL GUIDELINES:
- **Direct VPS execution**: You are operating directly on the server.
- **Websites & Domains (Code-First by default)**: When the user mentions a domain name or website URL (e.g. `example.com`), ALWAYS check if this website is hosted on the connected VPS target(s) FIRST. Inspect web server configs (`/etc/nginx/sites-enabled/`, `/etc/nginx/conf.d/`, `/etc/apache2/`, docker compose, etc.) to locate its project root / source code path (e.g. `/var/www/...`, `/root/...`). Read, inspect, and edit the source code and config files directly on the server filesystem. Do NOT treat the website as an external black box or rely primarily on `curl` when you have direct server filesystem and source code access.
- **Running shell commands**: Always use the MCP tool `run_command` with the exact Linux shell command (e.g. `docker compose -f /root/OLDS/docker-compose.yml up -d`, `find /root/OLDS -type f`, `grep -rn 'foo' /path`). NEVER prepend `ssh` or attempt to run local SSH/SCP client commands. The MCP bridge handles the SSH transport automatically.
- **Reading files**: Use `read_file(path)` with absolute Linux paths (e.g. `/root/OLDS/OLDS_Studio/src/app.tsx`).
- **Writing / Editing files**: Use `write_file(path, content)` with the full updated file contents. Never use `cat << 'EOF'` or local temp files.
- **Chat response style**: Speak naturally to the user as an expert engineer working directly on their server. Do not output raw SSH connection strings, ports, or hostnames in normal conversation unless asked.
"#;
    let _ = fs::write(root.join("AGENTS.md"), instructions);
    let _ = fs::write(root.join("GEMINI.md"), instructions);

    Ok(root)
}

/// Backwards compatibility alias for prepare_agent_workspace.
pub fn prepare_cursor_workspace(
    data_dir: &Path,
    session_id: &str,
    targets: &[String],
    safety: &str,
    workspace_id: &str,
) -> Result<PathBuf, String> {
    prepare_agent_workspace(data_dir, session_id, targets, safety, workspace_id)
}


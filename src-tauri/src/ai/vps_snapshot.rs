//! Collect live read-only snapshots from selected VPS targets for CLI providers
//! (Cursor/Codex/OpenCode) that cannot call xConsole's SSH tools directly.

use crate::ai::provider::{emit, ChatMessage, EventSink, StreamEvent};
use crate::ai::tools::{self, ToolContext};

/// Short label for snapshot bundle shown in the activity feed.
pub const SNAPSHOT_ACTIVITY_CMD: &str = "uptime; date; free -h; df -h; ps aux --sort=-%mem | head -20";

/// Read-only bundle run on each target before handing context to a CLI provider.
const SNAPSHOT_CMD: &str = "echo '=== uptime ==='; uptime; \
echo '=== date/time ==='; (date; timedatectl 2>/dev/null) 2>/dev/null; \
echo '=== memory ==='; (free -h 2>/dev/null || free) 2>/dev/null; \
echo '=== disk ==='; (df -h 2>/dev/null || df) 2>/dev/null; \
echo '=== top processes ==='; (ps aux --sort=-%mem 2>/dev/null || ps aux) 2>/dev/null | head -20; \
echo '=== systemd (running) ==='; (systemctl list-units --type=service --state=running --no-pager 2>/dev/null | head -25) 2>/dev/null; \
echo '=== listening ports ==='; (ss -tlnp 2>/dev/null || netstat -tlnp 2>/dev/null) 2>/dev/null | head -20";

/// Broad health/status questions — warrant the full SSH snapshot bundle.
const SNAPSHOT_KEYWORDS: &[&str] = &[
    "vps",
    "server",
    "servers",
    "host",
    "hosts",
    "node",
    "nodes",
    "running",
    "process",
    "processes",
    "port",
    "ports",
    "service",
    "services",
    "systemd",
    "container",
    "containers",
    "memory",
    "ram",
    "cpu",
    "load",
    "disk",
    "space",
    "date",
    "time",
    "timezone",
    "clock",
    "timedatectl",
    "ntp",
    "uptime",
    "reboot",
    "rebooted",
    "boot",
    "booted",
    "log",
    "logs",
    "ssh",
    "deploy",
    "deployment",
    "restart",
    "inspect",
    "monitor",
    "listening",
    "snapshot",
    "what is running",
    "what's running",
    "show me",
    "check the",
    "check my",
    "health",
    "status",
];

/// Package/service names for targeted install-or-version checks (not full snapshots).
const PACKAGE_NAMES: &[&str] = &[
    "nginx",
    "apache",
    "httpd",
    "docker",
    "mysql",
    "mariadb",
    "postgres",
    "postgresql",
    "redis",
    "node",
    "python",
    "php",
    "certbot",
];

/// Casual chat that should not trigger SSH prefetch or heavy tool schemas.
pub fn is_casual_chat(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    if lower.is_empty() {
        return true;
    }

    const EXACT: &[&str] = &[
        "hi",
        "hi!",
        "hi again",
        "hi again!",
        "hello",
        "hello!",
        "hello again",
        "hello again!",
        "hey",
        "hey!",
        "hey again",
        "hey again!",
        "yo",
        "sup",
        "hiya",
        "thanks",
        "thank you",
        "thx",
        "ok",
        "okay",
        "bye",
        "goodbye",
        "nice",
        "cool",
        "great",
    ];
    if EXACT.contains(&lower.as_str()) {
        return true;
    }

    if lower.ends_with(" again") {
        let base = lower.trim_end_matches(" again").trim();
        if matches!(base, "hi" | "hello" | "hey" | "yo" | "sup" | "hiya") {
            return true;
        }
    }

    const PHRASES: &[&str] = &[
        "how are you",
        "how're you",
        "how r u",
        "how's it going",
        "how is it going",
        "what's up",
        "whats up",
        "good morning",
        "good evening",
        "good night",
        "how do you do",
    ];
    PHRASES.iter().any(|p| lower.contains(p))
        || lower.contains("what model")
        || lower.contains("what llm")
        || lower.contains("who are you")
        || lower.contains("are you sure")
}

/// Coding help in chat — no VPS prefetch unless they mention a server.
pub fn is_local_coding_request(message: &str) -> bool {
    if is_casual_chat(message) {
        return false;
    }
    let lower = message.trim().to_lowercase();
    const VPS_HINTS: &[&str] = &[
        "vps",
        "server",
        "servers",
        "ssh",
        "on the server",
        "on my server",
        "on vps",
        "deploy to",
        "write to",
        "create on",
        "put on",
        "upload to",
        "both vps",
        "target",
        "remote",
    ];
    if VPS_HINTS.iter().any(|h| lower.contains(h)) {
        return false;
    }
    const CODING: &[&str] = &[
        "hello world",
        "python script",
        "javascript",
        "typescript",
        "make a script",
        "make an script",
        "write a script",
        "create a script",
        "example code",
        "sample code",
        "snippet",
        "function that",
        "program that",
        "code for",
        "can you make",
        "can you write",
        "show me code",
    ];
    CODING.iter().any(|c| lower.contains(c))
        || (lower.contains("script") && (lower.contains("python") || lower.contains("bash")))
}

fn is_package_install_query(lower: &str) -> bool {
    const INSTALL_PHRASES: &[&str] = &[
        "installed",
        "install ",
        "do we have",
        "have we got",
        "is there",
        "got ",
        "version of",
        "which version",
        "is nginx",
        "is docker",
        "is python",
        "is node",
        "running nginx",
        "running docker",
    ];
    INSTALL_PHRASES.iter().any(|p| lower.contains(p))
}

/// Whether the user asked about multiple servers (both/all/each/two).
pub fn user_asks_multiple_targets(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    // Bare "both" is usually confirming a prior offer (do both options), not "both servers".
    if matches!(lower.as_str(), "both" | "both." | "both!" | "both please") {
        return false;
    }
    const PHRASES: &[&str] = &[
        "both vps",
        "both server",
        "all vps",
        "all server",
        "all host",
        "each vps",
        "each server",
        "every vps",
        "every server",
        "two vps",
        "2 vps",
        "on both",
        "check both",
    ];
    PHRASES.iter().any(|p| lower.contains(p))
}

/// Short confirmations / menu picks after the assistant offered actions.
pub fn is_follow_up_approval(message: &str) -> bool {
    let lower = message.trim().to_lowercase();
    matches!(
        lower.as_str(),
        "both"
            | "both."
            | "both!"
            | "yes"
            | "yeah"
            | "yep"
            | "y"
            | "ok"
            | "okay"
            | "sure"
            | "go ahead"
            | "do it"
            | "please"
            | "1"
            | "2"
            | "1."
            | "2."
    ) || lower.starts_with("yes ")
        || lower.starts_with("ok ")
        || lower == "both please"
}

/// Resolve what the user wants this turn (handles short follow-ups like "both" / "yes").
pub fn effective_user_intent(messages: &[ChatMessage]) -> String {
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    if !is_follow_up_approval(last_user) {
        return last_user.to_string();
    }
    if let Some(prev_user) = messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .nth(1)
    {
        if should_collect(&prev_user.content) {
            return prev_user.content.clone();
        }
    }
    last_user.to_string()
}

/// Targeted question about one package/service (e.g. "do we have nginx installed?").
pub fn is_targeted_check(message: &str) -> bool {
    if is_casual_chat(message) || is_local_coding_request(message) {
        return false;
    }
    let lower = message.trim().to_lowercase();
    if !is_package_install_query(&lower) {
        return false;
    }
    let mentions_package = PACKAGE_NAMES.iter().any(|p| lower.contains(p));
    if !mentions_package {
        return false;
    }
    let broad = [
        "memory",
        "ram",
        "disk",
        "uptime",
        "reboot",
        "what is running",
        "what's running",
        "check my",
        "check the server",
        "listening port",
    ];
    !broad.iter().any(|b| lower.contains(b))
}

/// Full multi-section snapshot — not for single-package install checks.
pub fn should_collect_snapshot(last_user_message: &str) -> bool {
    if is_casual_chat(last_user_message)
        || is_targeted_check(last_user_message)
        || is_local_coding_request(last_user_message)
    {
        return false;
    }
    let lower = last_user_message.trim().to_lowercase();
    SNAPSHOT_KEYWORDS.iter().any(|kw| lower.contains(kw))
        || (lower.contains("docker") && !lower.contains("installed") && !lower.contains("version"))
}

/// Whether this turn needs live SSH data (direct server question or follow-up approval).
pub fn needs_live_data(messages: &[ChatMessage]) -> bool {
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    if is_local_coding_request(last_user) {
        return false;
    }
    if should_collect(last_user) {
        return true;
    }
    if !is_follow_up_approval(last_user) {
        return false;
    }
    if messages
        .iter()
        .rev()
        .filter(|m| m.role == "user")
        .nth(1)
        .is_some_and(|m| should_collect(&m.content))
    {
        return true;
    }
    false
}

/// Shell command to auto-run on selected targets based on user intent.
pub fn infer_live_command(messages: &[ChatMessage]) -> Option<String> {
    let intent = effective_user_intent(messages);
    let lower = intent.to_lowercase();

    if let Some(pkg_cmd) = infer_package_command(&lower) {
        return Some(pkg_cmd);
    }

    if lower.contains("reboot") || lower.contains("uptime") || lower.contains("boot") {
        return Some("uptime; date '+%Y-%m-%d %H:%M:%S %Z'".into());
    }
    if lower.contains("process") {
        return Some("ps aux --sort=-%mem | head -20".into());
    }
    if lower.contains("ram") || lower.contains("memory") || lower.contains("disk") || lower.contains("space")
    {
        return Some("free -h; df -h".into());
    }
    if should_collect_snapshot(&intent) {
        return Some("uptime; free -h; df -h".into());
    }
    None
}

fn infer_package_command(lower: &str) -> Option<String> {
    if is_local_coding_request(lower) || !is_package_install_query(lower) {
        return None;
    }
    if lower.contains("nginx") {
        return Some(
            "which nginx 2>/dev/null && nginx -v 2>&1 || echo 'nginx: not installed'".into(),
        );
    }
    if lower.contains("apache") || lower.contains("httpd") {
        return Some(
            "command -v apache2 httpd 2>/dev/null; systemctl is-active apache2 httpd 2>/dev/null || true; \
             dpkg -l apache2 2>/dev/null | head -3 || rpm -q httpd 2>/dev/null || echo 'apache/httpd: not installed'"
                .into(),
        );
    }
    if lower.contains("docker") {
        return Some(
            "command -v docker && docker --version || echo 'docker: not installed'; \
             systemctl is-active docker 2>/dev/null || true"
                .into(),
        );
    }
    if lower.contains("mysql") || lower.contains("mariadb") {
        return Some(
            "command -v mysql mysqld 2>/dev/null; systemctl is-active mysql mariadb 2>/dev/null || true; \
             dpkg -l mysql-server mariadb-server 2>/dev/null | head -3 || echo 'mysql/mariadb: check manually'"
                .into(),
        );
    }
    if lower.contains("postgres") {
        return Some(
            "command -v psql 2>/dev/null; systemctl is-active postgresql 2>/dev/null || true; \
             dpkg -l postgresql 2>/dev/null | head -3 || echo 'postgresql: check manually'"
                .into(),
        );
    }
    if lower.contains("redis") {
        return Some(
            "command -v redis-cli 2>/dev/null; systemctl is-active redis redis-server 2>/dev/null || true; \
             dpkg -l redis-server 2>/dev/null | head -3 || echo 'redis: check manually'"
                .into(),
        );
    }
    if lower.contains("python") {
        return Some(
            "command -v python3 python 2>/dev/null; python3 --version 2>/dev/null || python --version 2>/dev/null || echo 'python: not installed'"
                .into(),
        );
    }
    None
}

/// True when the auto command adds data beyond what the snapshot bundle already collects.
pub fn live_command_adds_beyond_snapshot(command: &str) -> bool {
    let c = command.to_lowercase();
    if c.contains("nginx")
        || c.contains("apache")
        || c.contains("httpd")
        || c.contains("docker --version")
        || c.contains("mysql")
        || c.contains("postgres")
        || c.contains("redis")
    {
        return true;
    }
    if c == "free -h; df -h" || c == "uptime; free -h; df -h" {
        return false;
    }
    if c.contains("uptime") && !c.contains(';') {
        return false;
    }
    if c.starts_with("ps aux") {
        return false;
    }
    !c.is_empty()
}

/// Auto-run a command on all selected targets; used when local models skip tool calls.
pub async fn collect_live_command(
    tc: &ToolContext,
    command: &str,
    sink: &EventSink,
) -> String {
    if tc.targets.is_empty() {
        return String::new();
    }
    let output = tools::run_command_all_targets(tc, command, sink).await;
    format!(
        "# Live command output (already executed on your VPS — do NOT say you will run commands)\n\n\
         Summarize the INTERPRETATION lines and uptime output below. \
         Never invent calendar reboot dates.\n\n```\n{output}\n```"
    )
}

/// Add human-readable facts local models often get wrong (uptime duration, etc.).
pub fn annotate_command_output(command: &str, raw: &str) -> String {
    let mut out = raw.to_string();
    let cmd_lower = command.to_lowercase();
    if cmd_lower.contains("uptime") || cmd_lower.contains("reboot") || cmd_lower.contains("boot") {
        if let Some(facts) = interpret_uptime_from_output(raw) {
            out.push_str("\nINTERPRETATION: ");
            out.push_str(&facts);
            out.push('\n');
        }
    }
    out
}

fn interpret_uptime_from_output(raw: &str) -> Option<String> {
    let text = if let Some(stdout) = raw.split("stdout:").nth(1) {
        stdout.to_string()
    } else {
        raw.to_string()
    };
    for line in text.lines() {
        let Some(idx) = line.find(" up ") else {
            continue;
        };
        let after = line[idx + 4..].trim();
        let end = after.find(',').unwrap_or(after.len());
        let duration = after[..end].trim();
        if duration.is_empty() {
            continue;
        }
        return Some(format!(
            "Server has been running for {duration} since the last reboot (from uptime line: \"{}\"). \
             Report this duration to the user — do NOT convert to a calendar date or guess days incorrectly.",
            line.trim()
        ));
    }
    None
}

/// System-prompt note when the user's wording doesn't match selected target count.
pub fn target_selection_note(message: &str, selected: usize) -> Option<String> {
    if !user_asks_multiple_targets(message) {
        return None;
    }
    if selected >= 2 {
        return Some(format!(
            "# Target selection\n\
             User asked about multiple servers. {selected} targets are selected — \
             run_command_all already covers all of them; summarize every section in the tool output."
        ));
    }
    Some(
        "# Target selection\n\
         User asked about both/all servers but only ONE target is selected in the agent panel. \
         Report results for that target only. Tell the user clearly that the other VPS is not \
         selected — they must add it in the agent target picker to include it. \
         Do not claim you checked or will check servers that are not selected."
            .to_string(),
    )
}

/// Whether this turn needs any SSH prefetch (snapshot and/or targeted command).
pub fn should_collect(last_user_message: &str) -> bool {
    should_collect_snapshot(last_user_message) || is_targeted_check(last_user_message)
}

/// SSH to each selected target, run a read-only snapshot, return markdown for the system prompt.
pub async fn collect(tc: &ToolContext, sink: &EventSink) -> String {
    if tc.targets.is_empty() {
        return String::new();
    }

    let mut sections: Vec<String> = Vec::new();

    for vps_id in &tc.targets {
        let Some(vps) = tc.db.get_vps(vps_id).ok().flatten() else {
            sections.push(format!("## unknown target ({vps_id})\nerror: VPS not found in database"));
            continue;
        };

        let activity_id = format!("snap-{vps_id}");
        tools::emit_command_activity_public(
            tc,
            sink,
            &activity_id,
            vps_id,
            SNAPSHOT_ACTIVITY_CMD,
        );

        let output = tools::exec_on_vps_quiet(tc, vps_id, SNAPSHOT_CMD).await;
        let annotated = annotate_command_output(SNAPSHOT_ACTIVITY_CMD, &output);
        tools::emit_command_result_public(sink, &activity_id, &annotated);

        emit(
            Some(sink),
            StreamEvent::ToolResult {
                id: format!("snapshot-{vps_id}"),
                output: annotated.clone(),
            },
        );

        sections.push(format!(
            "## {} (`{vps_id}`) — {}@{}:{}\n```\n{}\n```",
            vps.name,
            vps.username,
            vps.host,
            vps.port,
            annotated.trim_end()
        ));
    }

    if sections.is_empty() {
        return String::new();
    }

    format!(
        "# VPS snapshot (live data collected via SSH just now)\n\n\
         The user selected {} target(s). Answer using data from EVERY section below — \
         do not stop after the first server.\n\n{}",
        sections.len(),
        sections.join("\n\n")
    )
}

/// Trim snapshot markdown so it fits in smaller Ollama context windows.
/// Budget is split evenly across VPS sections so no target is dropped entirely.
pub fn truncate_for_context(snapshot: &str, num_ctx: u32) -> String {
    if snapshot.is_empty() {
        return String::new();
    }
    let max_chars = match num_ctx {
        0..=8192 => 3_000,
        8193..=16384 => 6_000,
        16385..=32768 => 10_000,
        32769..=65535 => 18_000,
        _ => 50_000,
    };
    if snapshot.len() <= max_chars {
        return snapshot.to_string();
    }

    const SECTION_MARKER: &str = "\n\n## ";
    let Some(first_section) = snapshot.find(SECTION_MARKER) else {
        return format!(
            "{}\n\n[... snapshot truncated for context window ({num_ctx} tokens) ...]",
            crate::ai::text::truncate_bytes(snapshot, max_chars)
        );
    };

    let (header, body) = snapshot.split_at(first_section);
    let mut sections: Vec<String> = body
        .split(SECTION_MARKER)
        .filter(|s| !s.is_empty())
        .map(|s| format!("## {s}"))
        .collect();

    if sections.is_empty() {
        return format!(
            "{}\n\n[... snapshot truncated for context window ({num_ctx} tokens) ...]",
            crate::ai::text::truncate_bytes(snapshot, max_chars)
        );
    }

    let header_len = header.len();
    let budget = max_chars.saturating_sub(header_len);
    let per_section = (budget / sections.len()).max(800);

    for section in &mut sections {
        if section.len() > per_section {
            *section = format!(
                "{}\n[... section truncated; use run_command_all for full output ...]",
                crate::ai::text::truncate_bytes(section, per_section)
            );
        }
    }

    format!(
        "{header}{}\n\n[Context limit: snapshot trimmed evenly across {} targets]",
        sections.join(SECTION_MARKER),
        sections.len()
    )
}

/// Compact system prompt for CLI providers (Cursor/Codex). Avoids the full SOUL
/// text that makes the CLI repeat a long introduction every turn.
pub fn build_cli_system(
    provider_label: &str,
    model_label: &str,
    target_count: usize,
    snapshot: &str,
    conversation_summary: Option<&str>,
) -> String {
    let mut parts = vec![
        "You are the xConsole DevOps copilot. Reply naturally — match the user's tone. \
         For casual chat (greetings, small talk), keep it brief and human; do not introduce \
         yourself or list capabilities. For server questions, be specific and technical. \
         Only mention VPS/server details when the user asked about servers or when live SSH \
         data appears below."
            .to_string(),
    ];
    if target_count > 0 && !snapshot.is_empty() {
        parts.push(format!(
            "The user selected {target_count} VPS target(s). Live SSH snapshot data is below — use it for this answer."
        ));
    } else if target_count > 0 {
        parts.push(format!(
            "The user has {target_count} VPS target(s) selected, but no live snapshot was collected for this message."
        ));
    }
    if !provider_label.is_empty() || !model_label.is_empty() {
        parts.push(format!("Provider: {provider_label}, model: {model_label}"));
    }
    if let Some(summary) = conversation_summary {
        if !summary.trim().is_empty() {
            parts.push(format!(
                "# This conversation (compact thread context)\n{}",
                summary.trim()
            ));
        }
    }
    if !snapshot.is_empty() {
        parts.push(snapshot.to_string());
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casual_chat_skips_snapshot() {
        assert!(!should_collect("hi"));
        assert!(!should_collect("hi again"));
        assert!(!should_collect("how are you"));
        assert!(!should_collect("Hey, how's it going?"));
    }

    #[test]
    fn server_questions_collect() {
        assert!(should_collect("what is running on my vps?"));
        assert!(should_collect("check memory on the servers"));
        assert!(should_collect("show me listening ports"));
    }

    #[test]
    fn python_script_is_local_not_vps_prefetch() {
        assert!(is_local_coding_request(
            "another problem can you make an python script with hello world?"
        ));
        assert!(!should_collect("another problem can you make an python script with hello world?"));
        assert!(!needs_live_data(&[ChatMessage::user(
            "another problem can you make an python script with hello world?"
        )]));
        assert!(infer_live_command(&[ChatMessage::user(
            "another problem can you make an python script with hello world?"
        )])
        .is_none());
    }

    #[test]
    fn python_on_vps_still_prefetches() {
        assert!(!is_local_coding_request("write a python script on my vps"));
        assert!(is_local_coding_request("make a python script with hello world"));
    }

    #[test]
    fn nginx_install_is_targeted_not_snapshot() {
        assert!(is_targeted_check("do we have nginx installed?"));
        assert!(should_collect("do we have nginx installed?"));
        assert!(!should_collect_snapshot("do we have nginx installed?"));
        let cmd = infer_live_command(&[ChatMessage::user("do we have nginx installed?")])
            .expect("nginx command");
        assert!(cmd.contains("nginx"));
        assert!(!cmd.contains("df -h"));
    }

    #[test]
    fn casual_yes_after_hi_does_not_prefetch() {
        use crate::ai::provider::ChatMessage;
        let msgs = vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant(
                "Anything you'd like to check? Nginx/SSH health status?",
            ),
            ChatMessage::user("yes"),
        ];
        assert!(!needs_live_data(&msgs));
        assert!(infer_live_command(&msgs).is_none());
    }

    #[test]
    fn multiple_target_phrases() {
        assert!(user_asks_multiple_targets("check both vps"));
        assert!(user_asks_multiple_targets("when did both reboot"));
        assert!(!user_asks_multiple_targets("both"));
        assert!(!user_asks_multiple_targets("check ram on my vps"));
    }

    #[test]
    fn follow_up_both_needs_live_data() {
        use crate::ai::provider::ChatMessage;
        let msgs = vec![
            ChatMessage::user("when did the vps last reboot check both"),
            ChatMessage::assistant("Would you like processes or disk?"),
            ChatMessage::user("both"),
        ];
        assert!(needs_live_data(&msgs));
        assert!(infer_live_command(&msgs).is_some());
    }

    #[test]
    fn uptime_interpretation() {
        let raw = "exit_code: 0\nstdout:\n 20:46:17 up 20:45,  1 user\n";
        let annotated = annotate_command_output("uptime", raw);
        assert!(annotated.contains("INTERPRETATION"));
        assert!(annotated.contains("20:45"));
    }

    #[test]
    fn selection_note_when_one_selected() {
        let note = target_selection_note("check both", 1).unwrap();
        assert!(note.contains("only ONE target"));
    }
}

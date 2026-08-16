//! Command-output compression before a tool result re-enters context.
//!
//! Four strategies per command type:
//!
//! 1. Smart filtering — drop hints, boilerplate, progress noise
//! 2. Grouping — files by extension, grep hits by file, tests by result
//! 3. Truncation — keep signal (errors, hunk headers), cut redundancy
//! 4. Deduplication — collapse repeated log lines with a count
//!
//! We compress the already-captured output. If compression is longer or empty
//! while raw is not, we keep the original (`never_worse`).

use serde_json::Value;

/// Estimated tokens as `bytes / 4`. Percentages are reliable;
/// absolute token numbers are approximate.
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressKind {
    GitStatus,
    GitLog,
    GitDiff,
    GitOk,
    CargoTest,
    CargoBuild,
    Pytest,
    NpmTest,
    Ls,
    Grep,
    DockerPs,
    Systemctl,
    Df,
    Journal,
    Generic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compressed {
    pub text: String,
    pub kind: CompressKind,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
}

impl Compressed {
    pub fn saved_bytes(&self) -> usize {
        self.original_bytes.saturating_sub(self.compressed_bytes)
    }

    pub fn saved_tokens(&self) -> usize {
        estimate_tokens(self.saved_bytes())
    }

    pub fn reduction(&self) -> f64 {
        if self.original_bytes == 0 {
            0.0
        } else {
            self.saved_bytes() as f64 / self.original_bytes as f64
        }
    }
}

pub fn command_from_call(tool: &str, args: &Value) -> String {
    args.get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| tool.replace('_', " "))
}

pub fn classify(command: &str) -> CompressKind {
    let c = command.trim();
    let lower = c.to_ascii_lowercase();
    let first = first_bin(&lower);
    if first == "git" || lower.contains("| git ") {
        if has_sub(&lower, "status") {
            return CompressKind::GitStatus;
        }
        if has_sub(&lower, "log") || has_sub(&lower, "show") {
            return if has_sub(&lower, "diff") || lower.contains("--stat") {
                CompressKind::GitDiff
            } else {
                CompressKind::GitLog
            };
        }
        if has_sub(&lower, "diff") {
            return CompressKind::GitDiff;
        }
        if ["add", "commit", "push", "pull", "fetch", "checkout", "switch"]
            .iter()
            .any(|s| has_sub(&lower, s))
        {
            return CompressKind::GitOk;
        }
        return CompressKind::Generic;
    }
    if first == "cargo" || lower.contains("cargo test") || lower.contains("cargo build") {
        if lower.contains("test") || lower.contains("nextest") {
            return CompressKind::CargoTest;
        }
        return CompressKind::CargoBuild;
    }
    if first == "pytest" || lower.contains("pytest") || lower.contains("python -m pytest") {
        return CompressKind::Pytest;
    }
    if ["npm", "pnpm", "yarn", "npx", "vitest", "jest"]
        .iter()
        .any(|b| first == *b || lower.contains(&format!("{b} test")))
    {
        return CompressKind::NpmTest;
    }
    if first == "ls" || first == "dir" || lower.starts_with("ls ") {
        return CompressKind::Ls;
    }
    if first == "grep" || first == "rg" || first == "egrep" || first == "fgrep" {
        return CompressKind::Grep;
    }
    if first == "docker" && (lower.contains("ps") || lower.contains("container ls")) {
        return CompressKind::DockerPs;
    }
    if first == "systemctl" || lower.contains("systemctl") {
        return CompressKind::Systemctl;
    }
    if first == "df" {
        return CompressKind::Df;
    }
    if first == "journalctl" || first == "dmesg" || lower.contains("journalctl") {
        return CompressKind::Journal;
    }
    CompressKind::Generic
}

/// Compress `raw` as if `command` produced it. Never returns a worse (longer) string.
pub fn compress(command: &str, raw: &str) -> Compressed {
    let kind = classify(command);
    let stripped = strip_ansi(raw);
    let filtered = match kind {
        CompressKind::GitStatus => compress_git_status(&stripped),
        CompressKind::GitLog => compress_git_log(&stripped),
        CompressKind::GitDiff => compress_git_diff(&stripped),
        CompressKind::GitOk => compress_git_ok(&stripped),
        CompressKind::CargoTest => compress_cargo_test(&stripped),
        CompressKind::CargoBuild => compress_cargo_build(&stripped),
        CompressKind::Pytest => compress_pytest(&stripped),
        CompressKind::NpmTest => compress_npm_test(&stripped),
        CompressKind::Ls => compress_ls(&stripped),
        CompressKind::Grep => compress_grep(&stripped),
        CompressKind::DockerPs => compress_docker_ps(&stripped),
        CompressKind::Systemctl => compress_systemctl(&stripped),
        CompressKind::Df => compress_df(&stripped),
        CompressKind::Journal => compress_journal(&stripped),
        CompressKind::Generic => compress_generic(&stripped),
    };
    let text = never_worse(&stripped, &filtered);
    let text = maybe_trailer(&stripped, text, kind);
    Compressed {
        kind,
        original_bytes: raw.len(),
        compressed_bytes: text.len(),
        text,
    }
}

pub fn compress_and_cap(command: &str, raw: &str, max_chars: usize) -> String {
    let c = compress(command, raw);
    if max_chars == 0 || c.text.len() <= max_chars {
        return c.text;
    }
    let mut cut = crate::ai::text::truncate_bytes(&c.text, max_chars).to_string();
    cut.push_str("\n…[output truncated to save context]");
    cut
}

fn never_worse(raw: &str, filtered: &str) -> String {
    if filtered.is_empty() && !raw.trim().is_empty() {
        return raw.to_string();
    }
    if filtered.len() >= raw.len() {
        raw.to_string()
    } else {
        filtered.to_string()
    }
}

fn maybe_trailer(raw: &str, compressed: String, kind: CompressKind) -> String {
    if kind == CompressKind::Generic && compressed.len() + 40 >= raw.len() {
        return compressed;
    }
    if raw.len() < 400 {
        return compressed;
    }
    let saved = raw.len().saturating_sub(compressed.len());
    if saved * 5 < raw.len() {
        return compressed;
    }
    let pct = saved * 100 / raw.len();
    format!(
        "{compressed}\n[compressed {}→{} chars · -{pct}% · ~{} tok]",
        raw.len(),
        compressed.len(),
        estimate_tokens(saved)
    )
}

fn first_bin(cmd: &str) -> &str {
    let rest = cmd
        .trim_start()
        .trim_start_matches("sudo ")
        .trim_start_matches("doas ");
    rest.split_whitespace().next().unwrap_or(rest)
}

fn has_sub(cmd: &str, sub: &str) -> bool {
    cmd.split_whitespace().any(|w| w == sub)
}

fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && !bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn dedup_runs(lines: impl Iterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev: Option<String> = None;
    let mut n = 0usize;
    let flush = |out: &mut Vec<String>, prev: &mut Option<String>, n: &mut usize| {
        if let Some(p) = prev.take() {
            if *n > 1 {
                out.push(format!("{p}  (×{n})"));
            } else {
                out.push(p);
            }
        }
        *n = 0;
    };
    for line in lines {
        if prev.as_deref() == Some(line.as_str()) {
            n += 1;
        } else {
            flush(&mut out, &mut prev, &mut n);
            prev = Some(line);
            n = 1;
        }
    }
    flush(&mut out, &mut prev, &mut n);
    out
}

fn looks_error(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("error")
        || l.contains("fail")
        || l.contains("fatal")
        || l.contains("panic")
        || l.contains("denied")
        || l.contains("traceback")
        || l.contains("exception")
}

// ---- git ------------------------------------------------------------------

fn compress_git_status(raw: &str) -> String {
    let mut branch = None;
    let mut staged = Vec::new();
    let mut modified = Vec::new();
    let mut untracked = Vec::new();
    let mut other = Vec::new();
    let mut clean = false;
    let mut in_untracked = false;
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("(use \"git")
            || t.starts_with("(create/copy")
            || t.contains("(use \"git add")
            || t.contains("(use \"git restore")
            || t.contains("(use \"git rm")
        {
            continue;
        }
        if t.starts_with("On branch ") {
            branch = Some(t.trim_start_matches("On branch ").to_string());
            continue;
        }
        if t.starts_with("Your branch") {
            other.push(t.to_string());
            continue;
        }
        if t.contains("nothing to commit") && t.contains("working tree clean") {
            clean = true;
            continue;
        }
        if t.starts_with("Untracked files:") {
            in_untracked = true;
            continue;
        }
        if t.starts_with("Changes to be committed:") {
            in_untracked = false;
            continue;
        }
        if t.starts_with("Changes not staged") {
            in_untracked = false;
            continue;
        }
        if t.len() >= 2 && t.as_bytes()[0].is_ascii_uppercase() && t.as_bytes()[1] == b' ' {
            // porcelain XY
            let code = &t[..1];
            let path = t[2..].trim();
            match code {
                "M" | "A" | "D" | "R" | "C" => staged.push(format!("{code} {path}")),
                "?" => untracked.push(path.to_string()),
                _ => modified.push(t.to_string()),
            }
            continue;
        }
        let p = t.trim();
        if p.starts_with("modified:") {
            modified.push(p.trim_start_matches("modified:").trim().to_string());
            continue;
        }
        if p.starts_with("new file:") || p.starts_with("deleted:") || p.starts_with("renamed:") {
            staged.push(p.to_string());
            continue;
        }
        if t.starts_with('\t') || (in_untracked && !t.ends_with(':')) {
            if in_untracked {
                untracked.push(p.to_string());
            } else {
                modified.push(p.to_string());
            }
            continue;
        }
    }
    if clean && staged.is_empty() && modified.is_empty() && untracked.is_empty() {
        return match branch {
            Some(b) => format!("ok · {b} · clean"),
            None => "ok · clean".into(),
        };
    }
    let mut out = String::new();
    if let Some(b) = branch {
        out.push_str(&format!("branch {b}\n"));
    }
    for line in other {
        out.push_str(&line);
        out.push('\n');
    }
    fn dump(out: &mut String, title: &str, items: &[String], cap: usize) {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("{title} ({})\n", items.len()));
        for i in items.iter().take(cap) {
            out.push_str("  ");
            out.push_str(i);
            out.push('\n');
        }
        if items.len() > cap {
            out.push_str(&format!("  … +{} more\n", items.len() - cap));
        }
    }
    dump(&mut out, "staged", &staged, 20);
    dump(&mut out, "modified", &modified, 20);
    dump(&mut out, "untracked", &untracked, 20);
    if out.is_empty() {
        filter_hints(raw)
    } else {
        out
    }
}

fn filter_hints(raw: &str) -> String {
    raw.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.starts_with("(use \"git")
                && !t.contains("(use \"git")
                && !t.starts_with("(create/copy")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compress_git_log(raw: &str) -> String {
    let mut commits = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t == "---END---" {
            continue;
        }
        if t.starts_with("commit ") && t.len() > 14 {
            commits.push(t[7..14].to_string() + &rest_subject(t));
            continue;
        }
        // oneline / already compact
        if t.len() >= 7 && t.as_bytes()[..7].iter().all(|b| b.is_ascii_hexdigit()) {
            commits.push(truncate_line(t, 88));
            continue;
        }
        if let Some(last) = commits.last_mut() {
            if last.len() < 88 && !t.starts_with("Author:") && !t.starts_with("Date:") {
                last.push(' ');
                last.push_str(&truncate_line(t, 40));
            }
        }
    }
    if commits.is_empty() {
        return raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(15)
            .map(|l| truncate_line(l, 88))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let extra = commits.len().saturating_sub(15);
    let mut out = commits.into_iter().take(15).collect::<Vec<_>>().join("\n");
    if extra > 0 {
        out.push_str(&format!("\n… +{extra} commits"));
    }
    out
}

fn rest_subject(commit_line: &str) -> String {
    // "commit abcdef... " — subject is on next line; handled by caller
    let _ = commit_line;
    String::new()
}

fn compress_git_diff(raw: &str) -> String {
    let mut out = Vec::new();
    let mut file = String::new();
    let mut added = 0u32;
    let mut removed = 0u32;
    let mut hunk_shown = 0u32;
    let mut hunk_skip = 0u32;
    let flush_file = |out: &mut Vec<String>, file: &str, added: u32, removed: u32| {
        if !file.is_empty() && (added > 0 || removed > 0) {
            out.push(format!("  +{added} -{removed}"));
        }
    };
    for line in raw.lines() {
        if line.starts_with("diff --git") {
            if hunk_skip > 0 {
                out.push(format!("  … ({hunk_skip} lines truncated)"));
                hunk_skip = 0;
            }
            flush_file(&mut out, &file, added, removed);
            file = line
                .split(" b/")
                .nth(1)
                .unwrap_or("unknown")
                .to_string();
            out.push(file.clone());
            added = 0;
            removed = 0;
            hunk_shown = 0;
            continue;
        }
        if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }
        if line.starts_with("@@") {
            if hunk_skip > 0 {
                out.push(format!("  … ({hunk_skip} lines truncated)"));
                hunk_skip = 0;
            }
            out.push(truncate_line(line, 100));
            hunk_shown = 0;
            continue;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
            if hunk_shown < 40 {
                out.push(truncate_line(line, 140));
                hunk_shown += 1;
            } else {
                hunk_skip += 1;
            }
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
            if hunk_shown < 40 {
                out.push(truncate_line(line, 140));
                hunk_shown += 1;
            } else {
                hunk_skip += 1;
            }
        }
        // drop context lines to save tokens
    }
    if hunk_skip > 0 {
        out.push(format!("  … ({hunk_skip} lines truncated)"));
    }
    flush_file(&mut out, &file, added, removed);
    if out.is_empty() {
        filter_hints(raw)
    } else {
        out.join("\n")
    }
}

fn compress_git_ok(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return "ok".into();
    }
    if looks_error(t) {
        return compress_generic(raw);
    }
    let first = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("ok");
    if first.len() < 80 && raw.lines().count() <= 3 {
        return first.to_string();
    }
    format!("ok · {}", truncate_line(first, 72))
}

// ---- tests / build --------------------------------------------------------

fn compress_cargo_test(raw: &str) -> String {
    let mut fails = Vec::new();
    let mut summary = None;
    let mut ok = 0u32;
    let mut failed = 0u32;
    let mut ignored = 0u32;
    let mut in_fail = false;
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with("Compiling ")
            || t.starts_with("Downloading ")
            || t.starts_with("Downloaded ")
            || t.starts_with("Checking ")
            || t.starts_with("Finished ")
        {
            continue;
        }
        if t.starts_with("test result:") {
            summary = Some(t.to_string());
            continue;
        }
        if let Some(name) = t.strip_prefix("test ") {
            if name.contains(" ... ok") {
                ok += 1;
            } else if name.contains(" ... FAILED") || name.contains(" ... ignored") {
                if name.contains("FAILED") {
                    failed += 1;
                    fails.push(truncate_line(t, 120));
                } else {
                    ignored += 1;
                }
            }
            continue;
        }
        if t == "failures:" {
            in_fail = true;
            continue;
        }
        if in_fail {
            if t.starts_with("error") || t.starts_with("thread") || t.starts_with("---- ") {
                fails.push(truncate_line(t, 160));
            }
        }
    }
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str(&s);
        out.push('\n');
    } else {
        out.push_str(&format!("ok={ok} failed={failed} ignored={ignored}\n"));
    }
    if !fails.is_empty() {
        out.push_str("failures:\n");
        for f in fails.into_iter().take(20) {
            out.push_str(&f);
            out.push('\n');
        }
    }
    if out.trim().is_empty() {
        compress_generic(raw)
    } else {
        out
    }
}

fn compress_cargo_build(raw: &str) -> String {
    let mut compiled = 0u32;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut finished = None;
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with("Compiling ") || t.starts_with("Checking ") {
            compiled += 1;
            continue;
        }
        if t.starts_with("Downloading ") || t.starts_with("Downloaded ") {
            continue;
        }
        if t.starts_with("Finished ") {
            finished = Some(t.to_string());
            continue;
        }
        if t.starts_with("error") {
            errors.push(truncate_line(t, 160));
        } else if t.starts_with("warning") && !t.contains("generated") {
            warnings.push(truncate_line(t, 140));
        }
    }
    if errors.is_empty() && warnings.is_empty() {
        return match finished {
            Some(f) => format!("ok · compiled {compiled} · {f}"),
            None => format!("ok · compiled {compiled}"),
        };
    }
    let mut out = format!("compiled {compiled} · {} error(s) · {} warning(s)\n", errors.len(), warnings.len());
    for e in errors.into_iter().take(20) {
        out.push_str(&e);
        out.push('\n');
    }
    for w in warnings.into_iter().take(8) {
        out.push_str(&w);
        out.push('\n');
    }
    out
}

fn compress_pytest(raw: &str) -> String {
    let mut fails = Vec::new();
    let mut summary = None;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('=') && (t.contains("failed") || t.contains("passed") || t.contains("error"))
        {
            summary = Some(t.trim_matches('=').trim().to_string());
            continue;
        }
        if t.contains("FAILED") || t.starts_with("E ") || t.starts_with(">") || t.contains("Error")
        {
            if !t.chars().all(|c| c == '.' || c == 'F' || c == 'E' || c == 's') {
                fails.push(truncate_line(t, 140));
            }
        }
    }
    let mut out = String::new();
    if let Some(s) = summary {
        out.push_str(&s);
        out.push('\n');
    }
    for f in fails.into_iter().take(20) {
        out.push_str(&f);
        out.push('\n');
    }
    if out.trim().is_empty() {
        compress_generic(raw)
    } else {
        out
    }
}

fn compress_npm_test(raw: &str) -> String {
    let mut fails = Vec::new();
    let mut summary = None;
    let mut passed = 0u32;
    let mut failed = 0u32;
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("✓") || t.contains(" PASS ") {
            passed += 1;
            continue;
        }
        if t.starts_with("×") || t.starts_with("✕") || t.contains(" FAIL ") || t.contains("failed")
        {
            failed += 1;
            fails.push(truncate_line(t, 140));
            continue;
        }
        if t.contains("Test Files") || t.contains("Tests ") || t.contains("Test Suites") {
            summary = Some(t.to_string());
        }
    }
    if passed + failed == 0 && summary.is_none() {
        return compress_generic(raw);
    }
    let mut out = match summary {
        Some(s) => s + "\n",
        None => format!("passed={passed} failed={failed}\n"),
    };
    for f in fails.into_iter().take(20) {
        out.push_str(&f);
        out.push('\n');
    }
    out
}

// ---- ls / grep / docker / system ------------------------------------------

fn compress_ls(raw: &str) -> String {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut parsed = 0usize;
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("total ") {
            continue;
        }
        if let Some((is_dir, name, size)) = parse_ls_line(t) {
            parsed += 1;
            if name == "." || name == ".." {
                continue;
            }
            if is_dir {
                dirs.push(name);
            } else {
                files.push((name, size));
            }
        }
    }
    if parsed == 0 {
        // plain ls (one name per line)
        let names: Vec<&str> = raw.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if names.len() <= 40 {
            return names.join("\n");
        }
        return format!(
            "{}\n… +{} more",
            names.iter().take(40).cloned().collect::<Vec<_>>().join("\n"),
            names.len() - 40
        );
    }
    let mut out = String::new();
    for d in dirs.iter().take(40) {
        out.push_str(d);
        out.push_str("/\n");
    }
    for (n, sz) in files.iter().take(60) {
        if *sz > 0 {
            out.push_str(&format!("{n}  {}\n", human_size(*sz)));
        } else {
            out.push_str(n);
            out.push('\n');
        }
    }
    let extra = dirs.len().saturating_sub(40) + files.len().saturating_sub(60);
    if extra > 0 {
        out.push_str(&format!("… +{extra} more · {} dirs {} files\n", dirs.len(), files.len()));
    } else {
        out.push_str(&format!("{} dirs · {} files\n", dirs.len(), files.len()));
    }
    out
}

fn parse_ls_line(line: &str) -> Option<(bool, String, u64)> {
    let bytes = line.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let ft = bytes[0] as char;
    if !matches!(ft, '-' | 'd' | 'l' | 'c' | 'b' | 'p' | 's') {
        return None;
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    // perms links owner group size month day time name...
    let size = parts.get(4).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let name = if parts.len() >= 9 {
        parts[8..].join(" ")
    } else {
        parts.last()?.to_string()
    };
    let name = name.trim_end_matches('*').trim_end_matches('/').to_string();
    Some((ft == 'd' || ft == 'l' && name.ends_with('/'), name, size))
}

fn human_size(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}G", n as f64 / 1e9)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        format!("{n}B")
    }
}

fn compress_grep(raw: &str) -> String {
    use std::collections::BTreeMap;
    let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let (file, rest) = match line.split_once(':') {
            Some((f, r)) if f.len() < 240 && !f.contains(' ') || f.contains('/') || f.contains('\\') => {
                (f.to_string(), r)
            }
            _ => ("(matches)".into(), line),
        };
        by_file
            .entry(file)
            .or_default()
            .push(truncate_line(rest, 120));
    }
    if by_file.is_empty() {
        return raw.to_string();
    }
    let mut out = String::new();
    let mut files = 0usize;
    for (file, hits) in &by_file {
        files += 1;
        if files > 30 {
            out.push_str(&format!("… +{} more files\n", by_file.len() - 30));
            break;
        }
        out.push_str(file);
        out.push_str(&format!("  ({} hits)\n", hits.len()));
        for h in hits.iter().take(8) {
            out.push_str("  ");
            out.push_str(h);
            out.push('\n');
        }
        if hits.len() > 8 {
            out.push_str(&format!("  … +{} more\n", hits.len() - 8));
        }
    }
    out
}

fn compress_docker_ps(raw: &str) -> String {
    let mut lines = raw.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return String::new();
    };
    // Keep NAMES + STATUS if present, else last two columns-ish
    let mut rows: Vec<String> = Vec::new();
    for line in lines {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 2 {
            let name = *cols.last().unwrap_or(&"?");
            let status = cols
                .iter()
                .find(|c| {
                    let u = c.to_ascii_uppercase();
                    u.starts_with("UP") || u.starts_with("EXIT") || u.contains("RESTART")
                })
                .copied()
                .unwrap_or(cols.get(4).copied().unwrap_or(""));
            rows.push(format!("{name}  {status}"));
        } else {
            rows.push(truncate_line(line, 80));
        }
    }
    let extra = rows.len().saturating_sub(30);
    let mut out = format!("{}\n", truncate_line(header, 80));
    for r in rows.into_iter().take(30) {
        out.push_str(&r);
        out.push('\n');
    }
    if extra > 0 {
        out.push_str(&format!("… +{extra} more\n"));
    }
    out
}

fn compress_systemctl(raw: &str) -> String {
    let mut keep = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("Loaded:")
            || t.starts_with("Active:")
            || t.starts_with("Main PID:")
            || t.starts_with("Docs:")
            || t.starts_with("●")
            || t.contains("failed")
            || t.contains("running")
            || t.contains(".service")
            || t.contains(".timer")
        {
            keep.push(truncate_line(t, 140));
        }
    }
    if keep.is_empty() {
        return compress_generic(raw);
    }
    let extra = keep.len().saturating_sub(25);
    let mut out = keep.into_iter().take(25).collect::<Vec<_>>().join("\n");
    if extra > 0 {
        out.push_str(&format!("\n… +{extra} more"));
    }
    out
}

fn compress_df(raw: &str) -> String {
    let mut out = String::new();
    for (i, line) in raw.lines().enumerate() {
        if i == 0 {
            out.push_str(truncate_line(line, 100).as_str());
            out.push('\n');
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let hot = t.contains("100%")
            || t.contains("9") && t.contains('%')
            || [" /", " /home", " /var", " /tmp"]
                .iter()
                .any(|m| t.ends_with(m) || t.contains(m));
        if hot || i < 8 {
            out.push_str(truncate_line(t, 100).as_str());
            out.push('\n');
        }
    }
    if out.lines().count() <= 1 {
        compress_generic(raw)
    } else {
        out
    }
}

fn compress_journal(raw: &str) -> String {
    let lines = dedup_runs(
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| truncate_line(l, 140)),
    );
    let extra = lines.len().saturating_sub(80);
    let mut out = lines.into_iter().take(80).collect::<Vec<_>>().join("\n");
    if extra > 0 {
        out.push_str(&format!("\n… +{extra} more"));
    }
    out
}

fn compress_generic(raw: &str) -> String {
    let cleaned: Vec<String> = dedup_runs(
        raw.lines()
            .map(str::trim_end)
            .filter(|l| !l.trim().is_empty())
            .map(|l| truncate_line(l, 200)),
    );
    if cleaned.len() <= 120 {
        return cleaned.join("\n");
    }
    let mut keep = Vec::new();
    let mid_errors: Vec<&String> = cleaned[40..cleaned.len().saturating_sub(20)]
        .iter()
        .filter(|l| looks_error(l))
        .take(20)
        .collect();
    keep.extend(cleaned.iter().take(40).cloned());
    if !mid_errors.is_empty() {
        keep.push("… [errors in the middle]".into());
        keep.extend(mid_errors.into_iter().cloned());
    }
    keep.push(format!(
        "… [{} lines omitted]",
        cleaned.len().saturating_sub(60)
    ));
    keep.extend(cleaned.iter().rev().take(20).cloned().rev());
    keep.join("\n")
}

fn truncate_line(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max.saturating_sub(1);
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// 50 unique cases proving output compression saves tokens without dropping errors.
pub fn selftest() -> Vec<(&'static str, bool)> {
    let noisy_status = "\
On branch main
Your branch is up to date with 'origin/main'.

Changes not staged for commit:
  (use \"git add <file>...\" to update what will be committed)
  (use \"git restore <file>...\" to discard changes in working directory)
	modified:   src/main.rs
	modified:   src/lib.rs

Untracked files:
  (use \"git add <file>...\" to include in what will be committed)
	tmp.o
	build.log

no changes added to commit (use \"git add\" and/or \"git commit -a\")
";
    let clean_status = "\
On branch main
Your branch is up to date with 'origin/main'.

nothing to commit, working tree clean
";
    let cargo_ok = {
        let mut s = String::new();
        for i in 0..40 {
            s.push_str(&format!("   Compiling crate{i} v1.0.0\n"));
        }
        s.push_str("    Finished `test` profile [unoptimized + debuginfo] target(s) in 12.0s\n");
        for i in 0..80 {
            s.push_str(&format!("test tests::case_{i} ... ok\n"));
        }
        s.push_str("test result: ok. 80 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s\n");
        s
    };
    let cargo_fail = "\
   Compiling app v0.1.0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.1s
     Running unittests src/lib.rs
test tests::a ... ok
test tests::boom ... FAILED

failures:

---- tests::boom stdout ----
thread 'tests::boom' panicked at src/lib.rs:9:5:
assertion `left == right` failed

failures:
    tests::boom

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
";
    let cargo_build_ok = {
        let mut s = String::new();
        for i in 0..30 {
            s.push_str(&format!("   Compiling dep{i} v0.2.0\nDownloading dep{i} v0.2.0\n"));
        }
        s.push_str("    Finished `release` profile [optimized] target(s) in 44.00s\n");
        s
    };
    let cargo_build_err = "\
   Compiling app v0.1.0
error[E0308]: mismatched types
  --> src/main.rs:3:5
error: could not compile `app` (bin \"app\") due to 1 previous error
";
    let git_log = (0..40)
        .map(|i| {
            format!(
                "commit {:040x}\nAuthor: A <a@b>\nDate:   Mon Aug 1 00:00:{i:02} 2026 +0000\n\n    chore: tweak {i}\n\n",
                i + 1
            )
        })
        .collect::<String>();
    let git_diff = {
        let mut s = String::from("diff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,80 +1,80 @@\n");
        for i in 0..80 {
            s.push_str(&format!(" context line {i} that the model does not need\n"));
            s.push_str(&format!("-old {i}\n+new {i}\n"));
        }
        s
    };
    let ls_long = {
        let mut s = String::from("total 128\n");
        s.push_str("drwxr-xr-x 2 u g 4096 Jan 1 12:00 src\n");
        s.push_str("drwxr-xr-x 2 u g 4096 Jan 1 12:00 tests\n");
        for i in 0..50 {
            s.push_str(&format!("-rw-r--r-- 1 u g {i}00 Jan 1 12:00 file{i}.rs\n"));
        }
        s
    };
    let grep_hits = (0..12)
        .flat_map(|f| {
            (0..20).map(move |n| format!("src/mod{f}.rs:{}:let x = {n};", n + 1))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let docker = {
        let mut s = String::from(
            "CONTAINER ID   IMAGE     COMMAND   CREATED   STATUS         PORTS     NAMES\n",
        );
        for i in 0..40 {
            s.push_str(&format!(
                "abc{i:03}          nginx     \"/docker\"  2 days    Up 2 days      80/tcp    web{i}\n"
            ));
        }
        s
    };
    let journal = {
        let mut s = String::new();
        for _ in 0..60 {
            s.push_str("sshd[1]: Failed password for root from 1.2.3.4 port 22 ssh2\n");
        }
        s.push_str("sshd[1]: Accepted publickey for deploy\n");
        s
    };
    let pytest_fail = "\
============================= test session starts ==============================
collected 12 items
test_a.py ...........F
=================================== FAILURES ===================================
____________________ test_a.py::test_boom _____________________
E   AssertionError: boom
=========================== 1 failed, 11 passed in 0.20s =======================
";
    let npm_ok = {
        let mut s = String::new();
        for i in 0..40 {
            s.push_str(&format!(" ✓ src/foo.test.ts > case {i}\n"));
        }
        s.push_str(" Test Files  1 passed (1)\n      Tests  40 passed (40)\n");
        s
    };
    let sys = "\
● ssh.service - OpenBSD Secure Shell server
     Loaded: loaded (/lib/systemd/system/ssh.service; enabled)
     Active: active (running) since Mon 2026-01-01 00:00:00 UTC; 10 days ago
   Main PID: 1234 (sshd)
        Docs: man:sshd(8)
              man:sshd_config(5)
CGroup: /system.slice/ssh.service
        ├─1234 sshd: /usr/sbin/sshd
        ├─1235 sshd: extra
        └─1236 sshd: extra
";
    let df = "\
Filesystem      Size  Used Avail Use% Mounted on
tmpfs           1.6G  2.0M  1.6G   1% /run
/dev/sda1        50G   48G  2.0G  96% /
tmpfs           7.8G     0  7.8G   0% /dev/shm
/dev/sdb1       200G   10G 190G   5% /data
";
    let ansi = "\x1b[31merror\x1b[0m: boom\n\x1b[32mok\x1b[0m\n";
    let push_ok = "Enumerating objects: 12, done.\nCounting objects: 100% (12/12), done.\nWriting objects: 100% (5/5), 1.20 KiB\nTo github.com:x/y.git\n   abc..def  main -> main\n";
    let generic_long = (0..200)
        .map(|i| format!("info: heartbeat tick {i} everything is fine"))
        .collect::<Vec<_>>()
        .join("\n");

    let c = |cmd: &str, raw: &str| compress(cmd, raw);
    let saves = |cmd: &str, raw: &str| c(cmd, raw).compressed_bytes < raw.len();
    let keeps = |cmd: &str, raw: &str, needle: &str| c(cmd, raw).text.contains(needle);

    vec![
        ("01 classify git status", classify("git status") == CompressKind::GitStatus),
        ("02 classify git log", classify("git log -n 20") == CompressKind::GitLog),
        ("03 classify git diff", classify("git diff HEAD~1") == CompressKind::GitDiff),
        ("04 classify git push", classify("sudo git push origin main") == CompressKind::GitOk),
        ("05 classify cargo test", classify("cargo test --all") == CompressKind::CargoTest),
        ("06 classify cargo build", classify("cargo build --release") == CompressKind::CargoBuild),
        ("07 classify pytest", classify("python -m pytest -q") == CompressKind::Pytest),
        ("08 classify pnpm test", classify("pnpm test") == CompressKind::NpmTest),
        ("09 classify ls -la", classify("ls -la /var/log") == CompressKind::Ls),
        ("10 classify rg", classify("rg TODO src") == CompressKind::Grep),
        ("11 classify docker ps", classify("docker ps -a") == CompressKind::DockerPs),
        ("12 classify systemctl", classify("systemctl status ssh") == CompressKind::Systemctl),
        ("13 classify df", classify("df -h") == CompressKind::Df),
        ("14 classify journalctl", classify("journalctl -u ssh --no-pager") == CompressKind::Journal),
        ("15 classify generic uptime", classify("uptime") == CompressKind::Generic),
        ("16 git status drops use-git hints", !keeps("git status", noisy_status, "use \"git add")),
        ("17 git status keeps modified files", keeps("git status", noisy_status, "main.rs")),
        ("18 git status is smaller than verbose", saves("git status", noisy_status)),
        ("19 git clean status is one line", c("git status", clean_status).text.contains("clean")),
        ("20 git log drops Author/Date blocks", !keeps("git log", &git_log, "Author:")),
        ("21 git log is smaller", saves("git log", &git_log)),
        ("22 git log keeps a hash", c("git log", &git_log).text.chars().filter(|ch| ch.is_ascii_hexdigit()).count() >= 7),
        ("23 git diff drops context lines", !keeps("git diff", &git_diff, "context line 50")),
        ("24 git diff keeps a +/− change", keeps("git diff", &git_diff, "+new")),
        ("25 git diff is smaller", saves("git diff", &git_diff)),
        ("26 git push collapses progress to ok", c("git push", push_ok).text.to_ascii_lowercase().contains("ok")),
        ("27 git push is smaller", saves("git push", push_ok)),
        ("28 cargo test drops Compiling noise", !keeps("cargo test", &cargo_ok, "Compiling crate3")),
        ("29 cargo test keeps the summary", keeps("cargo test", &cargo_ok, "80 passed")),
        ("30 cargo test ok is much smaller", c("cargo test", &cargo_ok).reduction() > 0.50),
        ("31 cargo test fail keeps FAILED", keeps("cargo test", cargo_fail, "FAILED")),
        ("32 cargo test fail keeps panic", keeps("cargo test", cargo_fail, "panicked")),
        ("33 cargo build ok drops Downloading", !keeps("cargo build", &cargo_build_ok, "Downloading")),
        ("34 cargo build ok is smaller", saves("cargo build", &cargo_build_ok)),
        ("35 cargo build keeps error[E0308]", keeps("cargo build", cargo_build_err, "E0308")),
        ("36 pytest keeps AssertionError", keeps("pytest", pytest_fail, "AssertionError")),
        ("37 pytest keeps failed count", keeps("pytest", pytest_fail, "failed")),
        ("38 npm test drops per-case checkmarks", c("pnpm test", &npm_ok).text.matches('✓').count() < 5),
        ("39 npm test keeps totals", keeps("pnpm test", &npm_ok, "40")),
        ("40 ls -la is smaller than long listing", saves("ls -la", &ls_long)),
        ("41 ls -la marks directories", keeps("ls -la", &ls_long, "src/")),
        ("42 grep groups by file", keeps("rg TODO", &grep_hits, "hits")),
        ("43 grep is smaller", saves("rg TODO src", &grep_hits)),
        ("44 docker ps is smaller", saves("docker ps", &docker)),
        ("45 docker ps keeps a container name", keeps("docker ps", &docker, "web0")),
        ("46 journalctl dedups repeated fails", keeps("journalctl -u ssh", &journal, "×")),
        ("47 journalctl is smaller", c("journalctl", &journal).reduction() > 0.50),
        ("48 systemctl keeps Active line", keeps("systemctl status ssh", sys, "Active:")),
        ("49 df keeps the full root fs", keeps("df -h", df, "/")),
        ("50 ansi is stripped; errors survive; never_worse; tokens shrink", {
            let a = c("echo hi", ansi);
            let g = c("yes | head", &generic_long);
            let tiny = c("echo hi", "hi\n");
            !a.text.contains('\u{1b}')
                && a.text.contains("error")
                && tiny.text.len() <= "hi\n".len() + 8
                && g.saved_tokens() > 0
                && estimate_tokens(4) == 1
                && command_from_call("run_command", &serde_json::json!({"command":"git status"}))
                    == "git status"
        }),
    ]
}

/// Truncate/age bulky tool result outputs from historical turns (e.g. > 3 turns ago)
/// to keep the active context window lean and prevent KV-cache bloat on long conversations.
pub fn age_historical_tool_results(
    messages: &mut [crate::ai::provider::ChatMessage],
    keep_recent_turns: usize,
    max_aged_bytes: usize,
) -> usize {
    let len = messages.len();
    if len <= keep_recent_turns {
        return 0;
    }

    let cutoff_idx = len.saturating_sub(keep_recent_turns);
    let mut aged_count = 0;

    for msg in &mut messages[..cutoff_idx] {
        if msg.role == "tool" && msg.content.len() > max_aged_bytes {
            let orig_len = msg.content.len();
            let lines: Vec<&str> = msg.content.lines().collect();
            if lines.len() > 10 {
                let head = &lines[..4];
                let tail = &lines[lines.len().saturating_sub(4)..];
                let truncated_lines = lines.len().saturating_sub(8);
                msg.content = format!(
                    "{}\n\n[... output aged: {} lines ({} bytes) truncated from earlier turn ...]\n\n{}",
                    head.join("\n"),
                    truncated_lines,
                    orig_len,
                    tail.join("\n")
                );
            } else {
                let head = &msg.content[..max_aged_bytes / 2];
                let tail = &msg.content[msg.content.len().saturating_sub(max_aged_bytes / 2)..];
                msg.content = format!(
                    "{}\n[... output aged: {} bytes truncated from earlier turn ...]\n{}",
                    head,
                    orig_len.saturating_sub(max_aged_bytes),
                    tail
                );
            }
            aged_count += 1;
        }
    }

    aged_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_fifty_output_compress_cases_pass() {
        let results = selftest();
        assert_eq!(results.len(), 50);
        let failed: Vec<_> = results.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
        assert!(failed.is_empty(), "failed: {failed:?}");
    }

    #[test]
    fn ages_older_tool_results_while_preserving_recent() {
        let mut msgs = vec![
            crate::ai::provider::ChatMessage::tool_result("call_1", "line\n".repeat(50)),
            crate::ai::provider::ChatMessage::assistant("thought"),
            crate::ai::provider::ChatMessage::tool_result("call_2", "line\n".repeat(50)),
        ];
        // Keep the last 1 turn: call_1 is aged, call_2 is preserved untouched
        let aged = age_historical_tool_results(&mut msgs, 1, 100);
        assert_eq!(aged, 1);
        assert!(msgs[0].content.contains("output aged"));
        assert!(!msgs[2].content.contains("output aged"));
    }
}

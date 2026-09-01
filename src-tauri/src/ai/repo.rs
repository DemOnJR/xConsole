//! What state a project's repository is actually in.
//!
//! Several agents working the same repository is where autonomous work stops being an
//! optimisation and starts being a liability. The failures are specific and all of them
//! are silent: a run ends with changes never committed, so the work is gone the next
//! time anything is checked out; a branch is committed but never pushed, so it exists
//! on one machine; a pull request sits open for three weeks until the code around it
//! has moved and merging it is a rewrite.
//!
//! None of that is visible from inside a turn. An agent that finishes its task has no
//! reason to look, and the goal loop had nothing to look with. So this reports the
//! facts — uncommitted, unpushed, unmerged — from one shell script, and the loop and
//! the review both read them.

use serde::Serialize;

/// The state of one working tree.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RepoStatus {
    /// False when the path is not a git work tree at all.
    pub is_repo: bool,
    pub branch: String,
    /// Tracked files with changes, plus untracked ones. Work that exists only here.
    pub dirty_files: u32,
    pub untracked_files: u32,
    /// Commits made here and not pushed. Work that exists only on this machine.
    pub ahead: u32,
    /// Commits on the upstream this branch has not taken.
    pub behind: u32,
    /// Empty when the branch has no upstream — which means `git push` alone does
    /// nothing and the work is going nowhere.
    pub upstream: String,
    pub stashes: u32,
}

impl RepoStatus {
    /// Whether the read actually worked.
    ///
    /// `repo: yes` with no branch means the commands inside the script failed — and the
    /// counts then come back as 0, which is indistinguishable from a clean tree. Saying
    /// "clean" because the question could not be asked is the one answer that loses
    /// work, so it is refused.
    pub fn is_readable(&self) -> bool {
        self.is_repo && !self.branch.trim().is_empty()
    }

    /// Work that exists in exactly one place and would be lost with the machine.
    pub fn work_at_risk(&self) -> bool {
        self.is_repo
            && (self.dirty_files > 0 || self.untracked_files > 0 || self.ahead > 0 || self.stashes > 0)
    }

    /// Said in the order it has to be fixed, and never as "all good" when it is not.
    pub fn summary(&self) -> String {
        if !self.is_repo {
            return "not a git repository".into();
        }
        if !self.is_readable() {
            return "could not read the repository state — treat as unknown, not clean".into();
        }
        let mut parts = Vec::new();
        if self.dirty_files > 0 {
            parts.push(format!("{} uncommitted file(s)", self.dirty_files));
        }
        if self.untracked_files > 0 {
            parts.push(format!("{} untracked file(s)", self.untracked_files));
        }
        if self.stashes > 0 {
            // A stash is work somebody meant to come back to, and nobody ever does.
            parts.push(format!("{} stash(es)", self.stashes));
        }
        if self.ahead > 0 {
            parts.push(format!("{} unpushed commit(s)", self.ahead));
        }
        if self.behind > 0 {
            parts.push(format!("{} commit(s) behind {}", self.behind, self.upstream));
        }
        if self.upstream.is_empty() {
            parts.push("no upstream — `git push` alone would go nowhere".into());
        }
        if parts.is_empty() {
            return format!("{} is clean and pushed", self.branch);
        }
        format!("{}: {}", self.branch, parts.join(", "))
    }
}

/// One pull request, as the host's CLI reports it.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub author: String,
    pub branch: String,
    /// Days since it was last touched.
    pub idle_days: i64,
    pub draft: bool,
    /// "CLEAN" | "DIRTY" (conflicts) | "BLOCKED" | "UNKNOWN", as reported.
    pub mergeable: String,
}

impl PullRequest {
    /// A pull request nobody is going to merge as it stands.
    ///
    /// Two weeks is not a deadline, it is the point where the code around a change has
    /// usually moved enough that the review has to start again. Conflicts make it true
    /// immediately, whatever the age.
    pub fn is_stale(&self) -> bool {
        self.idle_days >= 14 || self.mergeable.eq_ignore_ascii_case("DIRTY")
    }

    pub fn line(&self) -> String {
        let mut flags = Vec::new();
        if self.draft {
            flags.push("draft".to_string());
        }
        if self.mergeable.eq_ignore_ascii_case("DIRTY") {
            flags.push("CONFLICTS".to_string());
        }
        if self.idle_days >= 14 {
            flags.push(format!("idle {}d", self.idle_days));
        }
        format!(
            "#{} {} ({}){}",
            self.number,
            self.title,
            self.author,
            if flags.is_empty() { String::new() } else { format!(" — {}", flags.join(", ")) }
        )
    }
}

/// One shell script, so the whole picture costs a single round trip.
///
/// Written to print `key: value` lines rather than porcelain, because the caller wants
/// six numbers and parsing porcelain for them means reimplementing git's output format
/// and re-breaking on the next version.
pub fn status_command(dir: &str) -> String {
    let q = crate::ssh::remote_ops::shell_quote(dir);
    // `$( )` opens a fresh quoting context, so `"$d"` inside it needs no escaping.
    // Escaping it produced a literal backslash-quote, every substitution failed, and
    // `grep -c .` on the empty result printed 0 — a broken read that looked exactly
    // like a clean tree. Found by running it rather than by reading it.
    format!(
        "d={q}; \
         git -C \"$d\" rev-parse --is-inside-work-tree >/dev/null 2>&1 || {{ echo 'repo: no'; exit 0; }}; \
         echo 'repo: yes'; \
         echo \"branch: $(git -C \"$d\" rev-parse --abbrev-ref HEAD 2>/dev/null)\"; \
         echo \"dirty: $(git -C \"$d\" status --porcelain --untracked-files=no 2>/dev/null | grep -c .)\"; \
         echo \"untracked: $(git -C \"$d\" ls-files --others --exclude-standard 2>/dev/null | grep -c .)\"; \
         echo \"stashes: $(git -C \"$d\" stash list 2>/dev/null | grep -c .)\"; \
         up=$(git -C \"$d\" rev-parse --abbrev-ref --symbolic-full-name '@{{u}}' 2>/dev/null); \
         echo \"upstream: $up\"; \
         if [ -n \"$up\" ]; then \
           echo \"ahead: $(git -C \"$d\" rev-list --count \"$up\"..HEAD 2>/dev/null)\"; \
           echo \"behind: $(git -C \"$d\" rev-list --count HEAD..\"$up\" 2>/dev/null)\"; \
         else \
           echo 'ahead: 0'; echo 'behind: 0'; \
         fi"
    )
}

/// Parse what [`status_command`] printed.
///
/// Pure, so every field can be tested without a repository. Missing lines leave their
/// defaults rather than failing the whole read: a partial answer about a working tree
/// is worth more than none.
pub fn parse_status(stdout: &str) -> RepoStatus {
    let mut s = RepoStatus::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "repo" => s.is_repo = value == "yes",
            "branch" => s.branch = value.to_string(),
            "dirty" => s.dirty_files = value.parse().unwrap_or(0),
            "untracked" => s.untracked_files = value.parse().unwrap_or(0),
            "stashes" => s.stashes = value.parse().unwrap_or(0),
            "upstream" => s.upstream = value.to_string(),
            "ahead" => s.ahead = value.parse().unwrap_or(0),
            "behind" => s.behind = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    s
}

/// Ask the host's CLI for open pull requests. Works for GitHub (`gh`) and GitLab
/// (`glab`); prints nothing when neither is installed, which is not an error.
pub fn pr_list_command(dir: &str) -> String {
    let q = crate::ssh::remote_ops::shell_quote(dir);
    format!(
        "d={q}; cd \"$d\" 2>/dev/null || exit 0; \
         if command -v gh >/dev/null 2>&1; then \
           gh pr list --state open --limit 30 \
             --json number,title,author,headRefName,updatedAt,isDraft,mergeable 2>/dev/null; \
         elif command -v glab >/dev/null 2>&1; then \
           glab mr list --output json 2>/dev/null; \
         fi"
    )
}

/// Parse the JSON either CLI prints. Unknown shapes yield nothing rather than an error:
/// no pull requests and "we could not ask" both mean there is nothing to act on here,
/// and the caller says which by whether the tool was found.
pub fn parse_pull_requests(stdout: &str, now: chrono::DateTime<chrono::Utc>) -> Vec<PullRequest> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
        return vec![];
    };
    let Some(items) = v.as_array() else { return vec![] };
    items
        .iter()
        .map(|p| {
            let updated = p
                .get("updatedAt")
                .or_else(|| p.get("updated_at"))
                .and_then(|u| u.as_str())
                .and_then(|u| chrono::DateTime::parse_from_rfc3339(u).ok())
                .map(|d| d.with_timezone(&chrono::Utc));
            PullRequest {
                number: p
                    .get("number")
                    .or_else(|| p.get("iid"))
                    .and_then(|n| n.as_i64())
                    .unwrap_or(0),
                title: p.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                author: p
                    .get("author")
                    .and_then(|a| a.get("login").or_else(|| a.get("username")))
                    .and_then(|l| l.as_str())
                    .unwrap_or("someone")
                    .to_string(),
                branch: p
                    .get("headRefName")
                    .or_else(|| p.get("source_branch"))
                    .and_then(|b| b.as_str())
                    .unwrap_or("")
                    .to_string(),
                idle_days: updated.map(|u| (now - u).num_days().max(0)).unwrap_or(0),
                draft: p
                    .get("isDraft")
                    .or_else(|| p.get("draft"))
                    .and_then(|d| d.as_bool())
                    .unwrap_or(false),
                mergeable: p
                    .get("mergeable")
                    .and_then(|m| m.as_str())
                    .unwrap_or("UNKNOWN")
                    .to_string(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

pub fn definitions() -> Vec<crate::ai::provider::ToolDef> {
    use serde_json::json;
    vec![
        crate::ai::provider::ToolDef {
            name: "repo_status".into(),
            description: "The state of a project's repository: which branch, what is uncommitted, what is committed but not pushed, and which pull requests are open or have gone stale. Check it before starting work — so you branch from something current instead of on top of somebody's half-finished tree — and before finishing, so nothing is left only on one machine."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."}
                }
            }),
        },
        crate::ai::provider::ToolDef {
            name: "repo_save".into(),
            description: "Commit everything outstanding and push it, on the branch the tree is already on. Call this whenever you are about to stop, hand over, or wait for something — never leave work uncommitted. It does not choose a branch or open a pull request: the branch you are on is the one the user set up, and moving the work elsewhere is how it ends up somewhere nobody looks."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "Commit message. Say what changed and why, not that an agent did it."},
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."}
                },
                "required": ["message"]
            }),
        },
    ]
}

pub fn is_repo_tool(name: &str) -> bool {
    matches!(name, "repo_status" | "repo_save")
}

/// Reading the state changes nothing; committing and pushing is a write that leaves the
/// machine, so plan mode withholds it.
pub fn tool_is_mutating(name: &str) -> bool {
    name == "repo_save"
}

/// The project a call is about: named, or the one open.
fn project_of(
    ctx: &crate::ai::tools::ToolContext,
    args: &serde_json::Value,
) -> Result<(String, String), String> {
    let all = ctx.db.list_workspaces().unwrap_or_default();
    let named = args.get("project").and_then(|v| v.as_str()).map(str::trim);
    let ws = match named.filter(|n| !n.is_empty()) {
        Some(n) => all
            .iter()
            .find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n)
            .ok_or_else(|| format!("no project called {n:?}"))?,
        None => {
            let here = ctx
                .workspace_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("no project is open, so name one with `project`")?;
            all.iter().find(|w| w.id == here).ok_or("the open project no longer exists")?
        }
    };
    Ok((ws.id.clone(), ws.name.clone()))
}

pub async fn dispatch(
    ctx: &crate::ai::tools::ToolContext,
    name: &str,
    args: &serde_json::Value,
) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    match name {
        "repo_status" => {
            let status = match status_of(&ctx.db, &ctx.sessions, &ws_id).await {
                Ok(s) => s,
                Err(e) => return format!("error reading {ws_name}: {e}"),
            };
            let mut out = format!("{ws_name} — {}\n", status.summary());
            if status.work_at_risk() {
                out.push_str(
                    "This work exists in one place only. Commit and push it with repo_save \
                     before you stop.\n",
                );
            }
            let prs = pull_requests(&ctx.db, &ctx.sessions, &ws_id).await;
            let (stale, fresh): (Vec<_>, Vec<_>) = prs.iter().partition(|p| p.is_stale());
            out.push_str(&format!("\nOpen pull requests: {}\n", prs.len()));
            for p in fresh.iter().take(10) {
                out.push_str(&format!("- {}\n", p.line()));
            }
            if !stale.is_empty() {
                out.push_str(&format!("\nStale ({}) — deal with these first:\n", stale.len()));
                for p in stale.iter().take(10) {
                    out.push_str(&format!("- {}\n", p.line()));
                }
                out.push_str(
                    "A pull request left open long enough stops being reviewable: the code \
                     around it moves, and merging it becomes a rewrite. Rebase it and get it \
                     merged, or close it and say why — leaving it open is the one option \
                     that costs something later.\n",
                );
            }
            out
        }
        "repo_save" => {
            let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").trim();
            if message.is_empty() {
                return "error: repo_save needs a 'message'".into();
            }
            match save(&ctx.db, &ctx.sessions, &ws_id, message).await {
                Ok(out) => format!("{ws_name}: {}", out.trim()),
                Err(e) => format!(
                    "error: could not save {ws_name}: {e}\nThe work is still only where it \
                     was done. Fix this before moving on."
                ),
            }
        }
        _ => format!("error: unknown repo tool {name}"),
    }
}

/// Where a project's code lives, and how to reach it.
fn location(
    db: &crate::storage::Db,
    workspace_id: &str,
) -> Option<crate::ai::workspace_context::ProjectLocation> {
    db.get_workspace(workspace_id)
        .ok()
        .flatten()
        .and_then(|w| w.project_json)
        .and_then(|j| serde_json::from_str(&j).ok())
}

/// Run one of the scripts above wherever the project actually is.
async fn run_there(
    sessions: &crate::ssh::SessionManager,
    loc: &crate::ai::workspace_context::ProjectLocation,
    command: String,
) -> Result<String, String> {
    let path = loc.path.as_deref().filter(|p| !p.trim().is_empty());
    if path.is_none() {
        return Err("this project has no location set".into());
    }
    match loc.kind.as_str() {
        "vps" => {
            let vps = loc.vps_id.as_deref().ok_or("this project has no server set")?;
            sessions.run_command(vps, &command).await.map(|o| o.stdout)
        }
        _ => crate::proc::quiet_command("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .map_err(|e| e.to_string()),
    }
}

/// Read a project's repository state.
pub async fn status_of(
    db: &crate::storage::Db,
    sessions: &crate::ssh::SessionManager,
    workspace_id: &str,
) -> Result<RepoStatus, String> {
    let loc = location(db, workspace_id).ok_or("this project has no location set")?;
    let out = run_there(sessions, &loc, status_command(loc.path.as_deref().unwrap_or(""))).await?;
    Ok(parse_status(&out))
}

/// Open pull requests, when a host CLI is installed where the project lives.
pub async fn pull_requests(
    db: &crate::storage::Db,
    sessions: &crate::ssh::SessionManager,
    workspace_id: &str,
) -> Vec<PullRequest> {
    let Some(loc) = location(db, workspace_id) else { return vec![] };
    let cmd = pr_list_command(loc.path.as_deref().unwrap_or(""));
    match run_there(sessions, &loc, cmd).await {
        Ok(out) => parse_pull_requests(&out, chrono::Utc::now()),
        Err(_) => vec![],
    }
}

/// Commit and push whatever is outstanding. Returns what the script said.
pub async fn save(
    db: &crate::storage::Db,
    sessions: &crate::ssh::SessionManager,
    workspace_id: &str,
    message: &str,
) -> Result<String, String> {
    let loc = location(db, workspace_id).ok_or("this project has no location set")?;
    let cmd = save_command(loc.path.as_deref().unwrap_or(""), message);
    run_there(sessions, &loc, cmd).await
}

/// Commit everything and push it, on whatever branch the tree is on.
///
/// Deliberately not a policy about where work belongs. The branch the tree is already
/// on *is* the decision the user made — a task branch, `dev`, whatever they set up — and
/// inventing a different destination is how an agent's work ends up somewhere nobody
/// looks. Never force, never a different branch, never a merge.
pub fn save_command(dir: &str, message: &str) -> String {
    let d = crate::ssh::remote_ops::shell_quote(dir);
    let m = crate::ssh::remote_ops::shell_quote(message);
    format!(
        "d={d}; cd \"$d\" || exit 1; \
         git rev-parse --is-inside-work-tree >/dev/null 2>&1 || {{ echo 'not a git repository'; exit 1; }}; \
         b=$(git rev-parse --abbrev-ref HEAD 2>/dev/null); \
         [ \"$b\" = HEAD ] && {{ echo 'detached HEAD - refusing, a commit here belongs to no branch'; exit 1; }}; \
         git add -A || exit 1; \
         if git diff --cached --quiet; then echo 'nothing to commit'; else git commit -q -m {m} || exit 1; fi; \
         if git rev-parse --abbrev-ref --symbolic-full-name '@{{u}}' >/dev/null 2>&1; then \
           git push -q || exit 1; \
         else \
           git push -q -u origin \"$b\" || exit 1; \
         fi; \
         echo \"pushed $b\""
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&chrono::Utc)
    }

    #[test]
    fn a_clean_pushed_tree_says_so_and_nothing_more() {
        let s = parse_status(
            "repo: yes\nbranch: dev\ndirty: 0\nuntracked: 0\nstashes: 0\nupstream: origin/dev\nahead: 0\nbehind: 0",
        );
        assert!(!s.work_at_risk());
        assert_eq!(s.summary(), "dev is clean and pushed");
    }

    #[test]
    fn work_that_exists_in_only_one_place_is_flagged() {
        // Each of these is a way to lose everything an agent did, and each one is
        // invisible from inside the turn that created it.
        for line in ["dirty: 3", "untracked: 1", "ahead: 2", "stashes: 1"] {
            let s = parse_status(&format!(
                "repo: yes\nbranch: dev\nupstream: origin/dev\n{line}"
            ));
            assert!(s.work_at_risk(), "{line} should count as work at risk");
        }
    }

    #[test]
    fn a_branch_with_no_upstream_is_called_out() {
        // `git push` with no upstream fails, and an agent that ran it and moved on
        // believes the work is safe when it is on one disk.
        let s = parse_status("repo: yes\nbranch: feature/x\ndirty: 0\nuntracked: 0\nahead: 0\nupstream: ");
        assert!(s.summary().contains("no upstream"), "{}", s.summary());
    }

    #[test]
    fn a_path_that_is_not_a_repo_is_not_reported_as_clean() {
        let s = parse_status("repo: no");
        assert!(!s.work_at_risk());
        assert_eq!(s.summary(), "not a git repository");
    }

    #[test]
    fn a_garbled_read_does_not_claim_the_tree_is_fine() {
        // A truncated or failed command must not come back as "clean and pushed".
        let s = parse_status("");
        assert!(!s.is_repo);
        assert_eq!(s.summary(), "not a git repository");
    }

    #[test]
    fn a_read_that_half_worked_is_unknown_rather_than_clean() {
        // The real failure, found by running the script: bad quoting made every
        // substitution fail, so `grep -c .` printed 0 and the tree looked spotless
        // while holding uncommitted work. "Clean" is the one answer that loses work.
        let s = parse_status("repo: yes\nbranch: \ndirty: 0\nuntracked: 0\nahead: \nbehind:");
        assert!(!s.is_readable());
        assert!(s.summary().contains("could not read"), "{}", s.summary());
        assert!(!s.summary().contains("is clean and pushed"), "{}", s.summary());
    }

    #[test]
    #[cfg(unix)]
    fn the_generated_script_actually_runs() {
        // The quoting broke once and reading it did not show that: every substitution
        // failed, the counts came back 0, and the tree looked clean while holding
        // uncommitted work. So this runs it, against a repository built here.
        let dir = std::env::temp_dir().join(format!("xc-repo-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git runs")
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "one").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-qm", "first"]);
        // One tracked change and one untracked file: the two ways work goes missing.
        std::fs::write(dir.join("a.txt"), "two").unwrap();
        std::fs::write(dir.join("b.txt"), "new").unwrap();

        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(status_command(dir.to_str().unwrap()))
            .output()
            .expect("script runs");
        let parsed = parse_status(&String::from_utf8_lossy(&out.stdout));

        assert!(parsed.is_repo);
        assert!(parsed.is_readable(), "branch was empty: {parsed:?}");
        assert_eq!(parsed.branch, "main");
        assert_eq!(parsed.dirty_files, 1);
        assert_eq!(parsed.untracked_files, 1);
        // No remote, so there is no upstream - and that is worth saying, because a push
        // would go nowhere.
        assert!(parsed.upstream.is_empty());
        assert!(parsed.work_at_risk());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn saving_commits_and_refuses_a_detached_head() {
        let dir = std::env::temp_dir().join(format!("xc-save-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git runs")
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("a.txt"), "one").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-qm", "first"]);
        std::fs::write(dir.join("b.txt"), "work that must not be lost").unwrap();

        let run = |cmd: String| {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(&dir)
                .output()
                .expect("script runs")
        };

        // No remote here, so the push fails — but the commit must already have happened
        // by then. Committing first is what makes the work recoverable even when the
        // network or the remote is the thing that is broken.
        let _ = run(save_command(dir.to_str().unwrap(), "wip: agent task"));
        let after = parse_status(&String::from_utf8_lossy(
            &run(status_command(dir.to_str().unwrap())).stdout,
        ));
        assert_eq!(after.dirty_files, 0, "the change should be committed");
        assert_eq!(after.untracked_files, 0, "the new file should be committed");

        // A detached HEAD is refused rather than committed: a commit there belongs to no
        // branch and is lost at the next checkout, which is the exact failure being
        // guarded against.
        git(&["checkout", "-q", "--detach"]);
        std::fs::write(dir.join("c.txt"), "x").unwrap();
        let out = run(save_command(dir.to_str().unwrap(), "wip"));
        assert!(!out.status.success(), "detached HEAD must fail");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("detached"),
            "should say why: {}",
            String::from_utf8_lossy(&out.stdout)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pull_request_goes_stale_by_age_or_by_conflicting() {
        let now = utc("2026-08-30T00:00:00Z");
        let prs = parse_pull_requests(
            r#"[
              {"number":1,"title":"Fresh","author":{"login":"ada"},"headRefName":"a",
               "updatedAt":"2026-08-29T00:00:00Z","isDraft":false,"mergeable":"MERGEABLE"},
              {"number":2,"title":"Old","author":{"login":"bo"},"headRefName":"b",
               "updatedAt":"2026-08-01T00:00:00Z","isDraft":false,"mergeable":"MERGEABLE"},
              {"number":3,"title":"Clashing","author":{"login":"cy"},"headRefName":"c",
               "updatedAt":"2026-08-29T00:00:00Z","isDraft":false,"mergeable":"DIRTY"}
            ]"#,
            now,
        );
        assert_eq!(prs.len(), 3);
        assert!(!prs[0].is_stale());
        // Old enough that the code around it has moved.
        assert!(prs[1].is_stale());
        assert_eq!(prs[1].idle_days, 29);
        // Conflicting is stale immediately, whatever its age — nobody is merging it.
        assert!(prs[2].is_stale());
        assert!(prs[2].line().contains("CONFLICTS"));
    }

    #[test]
    fn a_gitlab_merge_request_parses_too() {
        // `glab` names the same things differently, and a team on GitLab should not get
        // an empty list that looks like "no open work".
        let prs = parse_pull_requests(
            r#"[{"iid":7,"title":"Fix","author":{"username":"dana"},
                 "source_branch":"fix/x","updated_at":"2026-08-20T00:00:00Z"}]"#,
            utc("2026-08-30T00:00:00Z"),
        );
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 7);
        assert_eq!(prs[0].author, "dana");
        assert_eq!(prs[0].idle_days, 10);
    }

    #[test]
    fn no_cli_installed_yields_nothing_rather_than_an_error() {
        assert!(parse_pull_requests("", chrono::Utc::now()).is_empty());
        assert!(parse_pull_requests("gh: command not found", chrono::Utc::now()).is_empty());
    }
}

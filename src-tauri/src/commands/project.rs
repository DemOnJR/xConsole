//! One project's history: what the agents did, said, changed and committed.
//!
//! # Why this exists
//!
//! The council is used across several projects at once. Before this, everything the
//! agents said landed in one undifferentiated pool: "what did the reviewer say" gave
//! three answers from three unrelated codebases, with nothing to tell them apart. The
//! fix has two halves — messages and delegated tasks are stamped with the project they
//! belong to (see the `workspace_id` columns), and this module is the other half: one
//! place to read a single project's record back.
//!
//! Four sources, assembled per project rather than four screens the user has to
//! correlate by timestamp: the tasks that were delegated, the conversation between the
//! agents, the files that changed, and the commits that came out of it.

use tauri::State;

use crate::ai::edits::EditRecord;
use crate::ai::workspace_context::ProjectLocation;
use crate::ssh::{shell_quote, SessionManager};
use crate::storage::models::{AgentMessage, GoalSession};
use crate::storage::Db;

/// One commit in the project's repository.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Commit {
    pub sha: String,
    pub author: String,
    /// ISO-8601, as git reports it.
    pub date: String,
    pub subject: String,
}

/// Everything one project has to show for itself.
#[derive(serde::Serialize)]
pub struct ProjectHistory {
    pub workspace_id: String,
    pub name: String,
    /// Where the project lives: a local folder or a path on a server.
    pub location: Option<String>,
    /// Delegated tasks, newest first.
    pub tasks: Vec<GoalSession>,
    /// What the agents said to each other, oldest first — it reads as a conversation.
    pub messages: Vec<AgentMessage>,
    /// Files the agents changed.
    pub changes: Vec<EditRecord>,
    pub branch: Option<String>,
    pub commits: Vec<Commit>,
    /// Why the git half is empty, when it is. Says "not a repository" rather than
    /// showing an empty list that could equally mean "no commits yet".
    pub git_note: Option<String>,
}

/// Field separator for the `git log` format.
///
/// A unit separator rather than a comma or a pipe: commit subjects contain both, and a
/// subject with a pipe in it would otherwise shift every field after it.
const SEP: &str = "\u{1f}";

fn log_command(dir: &str, limit: usize) -> String {
    let q = shell_quote(dir);
    format!(
        "d={q}; \
         git -C \"$d\" rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0; \
         git -C \"$d\" rev-parse --abbrev-ref HEAD 2>/dev/null; \
         git -C \"$d\" log -n {limit} --format='%h{SEP}%an{SEP}%aI{SEP}%s' 2>/dev/null"
    )
}

/// Parse the branch line plus the commit lines produced by [`log_command`].
///
/// Pure, so the shape of the output is testable without a repository or a server.
pub(crate) fn parse_log(stdout: &str) -> (Option<String>, Vec<Commit>) {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let Some(branch) = lines.next().map(str::to_string) else {
        return (None, vec![]);
    };
    let commits = lines
        .filter_map(|line| {
            let mut parts = line.split(SEP);
            let sha = parts.next()?.to_string();
            let author = parts.next()?.to_string();
            let date = parts.next()?.to_string();
            // The subject is last and may itself contain anything, so take the rest
            // rather than one more field.
            let subject = parts.collect::<Vec<_>>().join(SEP);
            (!sha.is_empty()).then_some(Commit { sha, author, date, subject })
        })
        .collect();
    (Some(branch), commits)
}

async fn read_git(
    sessions: &SessionManager,
    loc: &ProjectLocation,
    limit: usize,
) -> (Option<String>, Vec<Commit>, Option<String>) {
    let Some(path) = loc.path.as_deref().filter(|p| !p.trim().is_empty()) else {
        return (None, vec![], Some("this project has no location set".into()));
    };
    let out = match loc.kind.as_str() {
        "vps" => {
            let Some(vps) = loc.vps_id.as_deref() else {
                return (None, vec![], Some("this project has no server set".into()));
            };
            match sessions.run_command(vps, &log_command(path, limit)).await {
                Ok(o) => o.stdout,
                Err(e) => return (None, vec![], Some(format!("could not reach the server: {e}"))),
            }
        }
        _ => match crate::proc::quiet_command("sh").arg("-c").arg(log_command(path, limit)).output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(e) => return (None, vec![], Some(format!("could not run git: {e}"))),
        },
    };
    let (branch, commits) = parse_log(&out);
    let note = branch.is_none().then(|| "this project is not a git repository".to_string());
    (branch, commits, note)
}

#[tauri::command]
pub async fn project_history(
    db: State<'_, Db>,
    sessions: State<'_, SessionManager>,
    workspace_id: String,
    limit: Option<i64>,
) -> Result<ProjectHistory, String> {
    history(&db, &sessions, &workspace_id, limit.unwrap_or(200)).await
}

/// Assemble one project's record.
///
/// A plain function rather than only a command, so the agent's `project_history` tool
/// reads the same four sources the user's screen does — an agent catching up on a
/// project should not be looking at a different history from the person it reports to.
pub async fn history(
    db: &Db,
    sessions: &SessionManager,
    workspace_id: &str,
    limit: i64,
) -> Result<ProjectHistory, String> {
    let workspace_id = workspace_id.to_string();
    let ws = db
        .get_workspace(&workspace_id)
        .map_err(|e| e.to_string())?
        .ok_or("no such project")?;
    let loc: Option<ProjectLocation> = ws
        .project_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());

    let tasks = db
        .list_goals_for_workspace(Some(&workspace_id))
        .map_err(|e| e.to_string())?;
    let messages = db
        .list_agent_messages(None, Some(&workspace_id), limit)
        .map_err(|e| e.to_string())?;
    let changes = db
        .list_file_changes(None, Some(&workspace_id), limit)
        .map_err(|e| e.to_string())?;

    let (branch, commits, git_note) = match &loc {
        Some(loc) => read_git(sessions, loc, 30).await,
        None => (None, vec![], Some("this project has no location set".into())),
    };

    Ok(ProjectHistory {
        workspace_id,
        name: ws.name,
        location: loc.as_ref().and_then(|l| l.path.clone()),
        tasks,
        messages,
        changes,
        branch,
        commits,
        git_note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subject_containing_the_separator_keeps_its_tail() {
        // Commit subjects are free text. If the parser took a fixed field count, a
        // subject that happened to contain the separator would be silently truncated.
        let out = format!(
            "main\nabc123{SEP}Ada{SEP}2026-08-29T10:00:00+03:00{SEP}fix: a{SEP}b subject"
        );
        let (branch, commits) = parse_log(&out);
        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].sha, "abc123");
        assert_eq!(commits[0].author, "Ada");
        assert_eq!(commits[0].subject, format!("fix: a{SEP}b subject"));
    }

    #[test]
    fn a_non_repository_reports_no_branch_rather_than_an_empty_history() {
        // The shell script exits silently on a non-repo, so the two cases arrive as
        // the same empty-ish output and have to be told apart here: no branch line at
        // all means "not a repository", which the UI states instead of showing an empty
        // commit list that reads as "no commits yet".
        let (branch, commits) = parse_log("");
        assert!(branch.is_none());
        assert!(commits.is_empty());
    }

    #[test]
    fn a_repository_with_no_commits_still_reports_its_branch() {
        let (branch, commits) = parse_log("main\n");
        assert_eq!(branch.as_deref(), Some("main"));
        assert!(commits.is_empty());
    }

    #[test]
    fn malformed_commit_lines_are_skipped_not_fatal() {
        let out = format!("dev\nnot-a-commit-line\ndef456{SEP}Bo{SEP}2026-01-01T00:00:00Z{SEP}ok");
        let (_, commits) = parse_log(&out);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "ok");
    }
}

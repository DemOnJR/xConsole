//! One open pull request per agent, enforced instead of requested.
//!
//! "Do not leave a pull request open and idle" is written into the persona prompt, the
//! goal brief, the context header and the bundled gitops skill. It is prose in every one
//! of them, and prose is advice: an agent that opens a second PR has not violated
//! anything a machine can see, so nobody finds out until a human opens the repository
//! and finds four branches nobody merged.
//!
//! No code in xConsole creates a pull request. The only `gh` invocation in the app is
//! the read-only listing in [`crate::ai::repo`]. An agent opens a PR the same way a
//! person does — by typing `gh pr create` into `run_command` — which is exactly where
//! this can be intercepted, and the only place it can be.
//!
//! Two halves, both pure so they can be tested without a database or a shell:
//! [`is_pr_create`] decides whether a command line opens a pull request, and [`blocks`]
//! decides whether this agent is allowed to run it right now. The wiring reads the open
//! rows from `agent_open_pr` (see the accessors in [`crate::storage`]) and passes them
//! in.

// Unreachable until the guard band lands in `ai/tools.rs` — that file is owned by
// another change in this wave, so the rule is written, tested and wired to nothing.
// Remove this the moment it is called: after that, a dead function here is a real one.


use serde_json::Value;

/// A pull request an agent has open, as stored in `agent_open_pr`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenPr {
    pub persona_id: String,
    /// Empty when the agent was not working inside a workspace.
    pub workspace_id: String,
    pub branch: String,
    pub pr_number: Option<i64>,
    pub url: Option<String>,
    pub opened_at: String,
}

impl OpenPr {
    /// How to refer to it in one phrase: the number if we have it, else the URL, else
    /// the branch. Something always identifies it, because a refusal that cannot name
    /// what is blocking is a refusal the model will argue with.
    fn label(&self) -> String {
        match (self.pr_number, self.url.as_deref()) {
            (Some(n), _) => format!("#{n}"),
            (None, Some(u)) if !u.is_empty() => u.to_string(),
            _ => format!("the one on {}", self.branch),
        }
    }

    /// The command that clears the block. `gh pr merge` needs a number; without one the
    /// agent has to look it up first, and saying so beats inventing a number.
    fn clearing_action(&self) -> String {
        match self.pr_number {
            Some(n) => format!(
                "`gh pr merge {n} --squash --delete-branch` once it is reviewed, or \
                 `gh pr close {n}` if it should not land"
            ),
            None => format!(
                "`gh pr list --head {}` to find its number, then merge or close it",
                self.branch
            ),
        }
    }
}

/// The tools an agent can open a pull request through.
///
/// Both run a shell, and a shell is all `gh pr create` needs. Anything else in the tool
/// set edits files or reads them; none of it can reach GitHub.
pub fn tool_can_open_pr(tool: &str) -> bool {
    matches!(tool, "run_command" | "local_run_command" | "run_command_all")
}

/// Whether this command line opens a pull request.
///
/// Deliberately narrow. `gh pr list` and `gh pr view` are how an agent *checks* on the
/// PR it already has, and blocking those would leave it unable to discover the thing it
/// is being told to go and finish.
pub fn is_pr_create(command: &str) -> bool {
    segments(command).iter().any(|s| segment_opens_pr(s, 0))
}

/// Whether `persona` may run `tool` with `args` right now, and why not.
///
/// The returned string is the tool result the model reads next, so it is written to be
/// acted on rather than apologised for: what is open, where, and the single command that
/// unblocks the work.
pub fn blocks(open_prs: &[OpenPr], persona: &str, tool: &str, args: &Value) -> Option<String> {
    if !tool_can_open_pr(tool) {
        return None;
    }
    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() || !is_pr_create(command) {
        return None;
    }
    // An unnamed turn is the user driving the app by hand. The rule is about unattended
    // agents piling up branches nobody asked for; a person typing into the console is
    // not that, and blocking them would be the app refusing its own owner.
    if persona.trim().is_empty() {
        return None;
    }
    let existing = open_prs.iter().find(|p| p.persona_id == persona)?;

    Some(format!(
        "blocked: you already have a pull request open — {} on branch {}{}, opened {}. \
         One open PR at a time: a second one splits review across two branches and \
         neither gets merged.\n\n\
         Finish that one first — {}. When it is merged or closed, this command runs \
         without asking. If it is waiting on a reviewer, say so and stop rather than \
         opening another branch.",
        existing.label(),
        existing.branch,
        existing
            .url
            .as_deref()
            .filter(|u| !u.is_empty() && existing.pr_number.is_some())
            .map(|u| format!(" ({u})"))
            .unwrap_or_default(),
        if existing.opened_at.is_empty() {
            "earlier"
        } else {
            existing.opened_at.as_str()
        },
        existing.clearing_action(),
    ))
}

/// The pull request a successful `gh pr create` just opened: (branch, number, url).
///
/// `gh pr create` prints the PR URL on success and nothing resembling one on failure,
/// so the output is the only honest signal that a PR now exists. Parsing the command
/// alone would record a PR for every refused, rate-limited or mistyped attempt, and the
/// agent would then be blocked behind something that was never opened.
pub fn opened_pr(command: &str, output: &str) -> Option<(String, Option<i64>, String)> {
    let url = output
        .lines()
        .map(str::trim)
        .find(|l| l.contains("/pull/") || l.contains("/merge_requests/"))?
        .to_string();
    let number = url
        .rsplit('/')
        .next()
        .and_then(|n| n.trim().parse::<i64>().ok());
    // `--head` when the command names it; otherwise the number is the only stable
    // identity we have, and the ledger's primary key needs a branch.
    let branch = flag_value(command, "--head")
        .or_else(|| flag_value(command, "--source-branch"))
        .unwrap_or_else(|| {
            number
                .map(|n| format!("pr-{n}"))
                .unwrap_or_else(|| "unknown".to_string())
        });
    Some((branch, number, url))
}

/// The pull request a successful `gh pr merge` / `gh pr close` just ended.
///
/// Only what the agent does itself. A PR landed by a human in the GitHub UI leaves no
/// trace here, which is why the ledger is also reconciled against the real list when a
/// run finishes -- otherwise an agent stays wedged behind a PR that merged days ago.
pub fn closed_pr(command: &str, output: &str) -> Option<i64> {
    let seg = segments(command).into_iter().find(|s| {
        let words: Vec<&str> = s.split_whitespace().collect();
        let is_gh = words
            .iter()
            .any(|w| matches!(basename(unquote(w)), "gh" | "glab"));
        is_gh
            && words
                .windows(2)
                .any(|w| matches!((w[0], w[1]), ("pr", "merge") | ("pr", "close") | ("mr", "merge") | ("mr", "close")))
    })?;
    // A failed merge says so and changes nothing; only clear the ledger on success.
    let lower = output.to_lowercase();
    if lower.starts_with("error") || lower.contains("not mergeable") || lower.contains("failed to") {
        return None;
    }
    seg.split_whitespace()
        .map(unquote)
        .find_map(|w| w.trim_start_matches('#').parse::<i64>().ok())
}

/// The value following `flag` on a command line, unquoted.
fn flag_value(command: &str, flag: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    if let Some(v) = words
        .iter()
        .position(|w| *w == flag)
        .and_then(|i| words.get(i + 1))
    {
        let v = unquote(v);
        if !v.is_empty() && !v.starts_with('-') {
            return Some(v.to_string());
        }
    }
    // `--head=branch` is the same instruction written differently.
    words
        .iter()
        .find_map(|w| w.strip_prefix(&format!("{flag}=")))
        .map(|v| unquote(v).to_string())
        .filter(|v| !v.is_empty())
}

/// Split a command line into the pieces a shell would run separately.
///
/// Quote-aware, because `echo "a && b"` is one command and splitting it would invent a
/// second. Everything else about shell grammar is ignored on purpose: this decides
/// whether to look at a segment, not what the segment means.
fn segments(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '\\' => {
                    cur.push(c);
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                ';' | '\n' | '(' | ')' | '{' | '}' => {
                    out.push(std::mem::take(&mut cur));
                }
                '&' | '|' => {
                    // One or two of them separates commands either way.
                    if chars.peek() == Some(&c) {
                        chars.next();
                    }
                    out.push(std::mem::take(&mut cur));
                }
                _ => cur.push(c),
            },
        }
    }
    out.push(cur);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

/// Whether one segment is `gh pr create` (or the GitLab equivalent).
///
/// `depth` bounds the `bash -c '...'` unwrapping so a pathological nesting cannot spin.
fn segment_opens_pr(segment: &str, depth: usize) -> bool {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let mut i = 0;
    // Leading `sudo`, `env`, `VAR=value`, `time`, `nohup` are all noise in front of the
    // program name, and every one of them is something an agent actually types.
    while i < tokens.len() {
        let t = tokens[i];
        if matches!(t, "sudo" | "env" | "time" | "nohup" | "command" | "exec")
            || (t.contains('=') && !t.starts_with('-') && !t.contains('/'))
        {
            i += 1;
            continue;
        }
        break;
    }
    let Some(program) = tokens.get(i).map(|t| basename(t)) else {
        return false;
    };

    // `bash -c 'gh pr create ...'` hides the whole command inside one argument. Look
    // inside it once rather than pretending the wrapper is the command.
    if matches!(program, "bash" | "sh" | "zsh" | "dash" | "ksh") && depth < 2 {
        if let Some(script) = tokens[i..].iter().position(|t| *t == "-c").and_then(|p| {
            let rest = tokens[i + p + 1..].join(" ");
            let trimmed = rest.trim();
            Some(unquote(trimmed).to_string())
        }) {
            return segments(&script).iter().any(|s| segment_opens_pr(s, depth + 1));
        }
        return false;
    }

    let (namespace, verb) = match program {
        "gh" => ("pr", "create"),
        "glab" => ("mr", "create"),
        _ => return false,
    };

    // Flags and their values sit anywhere on a `gh` line (`gh --repo o/r pr create`), so
    // the subcommand is found by looking for the pair adjacent among the non-flag words
    // rather than at a fixed position. `gh pr list` and `gh pr view 3` do not match,
    // which is the point: those are how an agent checks the PR it already has.
    let words: Vec<&str> = tokens[i + 1..]
        .iter()
        .copied()
        .filter(|t| !t.starts_with('-'))
        .collect();
    words
        .windows(2)
        .any(|w| w[0] == namespace && w[1] == verb)
}

fn basename(token: &str) -> &str {
    let t = token.rsplit(['/', '\\']).next().unwrap_or(token);
    t.strip_suffix(".exe").unwrap_or(t)
}

fn unquote(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'\'' || b[0] == b'"') && b[b.len() - 1] == b[0] {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    /// The ledger must only gain a row when a PR actually exists. `gh pr create` can
    /// fail for a dozen reasons and says nothing that looks like a URL when it does.
    #[test]
    fn a_pull_request_is_recorded_only_when_the_command_printed_one() {
        let cmd = "gh pr create --head wip/ada/fake-detection --title x --body y";
        let (branch, number, url) = super::opened_pr(
            cmd,
            "https://github.com/o/r/pull/412\n",
        )
        .expect("a printed URL is a real pull request");
        assert_eq!(branch, "wip/ada/fake-detection");
        assert_eq!(number, Some(412));
        assert_eq!(url, "https://github.com/o/r/pull/412");

        assert!(
            super::opened_pr(cmd, "error: pull request already exists for this branch").is_none(),
            "a refused create must not wedge the agent behind a PR it never opened"
        );
    }

    /// Without `--head` there is still a row to write, and the primary key needs a
    /// branch. The number is the only stable identity left.
    #[test]
    fn a_create_without_an_explicit_branch_still_gets_an_identity() {
        let (branch, number, _) =
            super::opened_pr("gh pr create --fill", "https://github.com/o/r/pull/7").unwrap();
        assert_eq!(number, Some(7));
        assert_eq!(branch, "pr-7");
    }

    #[test]
    fn the_branch_can_be_written_either_way() {
        let (branch, _, _) = super::opened_pr(
            "gh pr create --head=wip/ada/x --fill",
            "https://github.com/o/r/pull/9",
        )
        .unwrap();
        assert_eq!(branch, "wip/ada/x");
    }

    /// Merging is how an agent clears its own block. A merge that failed clears nothing,
    /// or the next `gh pr create` would be allowed while the old PR is still open.
    #[test]
    fn merging_clears_the_ledger_but_only_when_it_worked() {
        assert_eq!(
            super::closed_pr("gh pr merge 412 --squash", "Merged pull request #412"),
            Some(412)
        );
        assert_eq!(
            super::closed_pr("gh pr close 412", "Closed pull request #412"),
            Some(412)
        );
        assert_eq!(
            super::closed_pr("gh pr merge 412 --squash", "error: Pull request is not mergeable"),
            None
        );
        assert_eq!(
            super::closed_pr("gh pr list --state open", "#412 something"),
            None,
            "listing is not closing"
        );
    }

    use super::*;
    use serde_json::json;

    fn pr(persona: &str, branch: &str, number: Option<i64>) -> OpenPr {
        OpenPr {
            persona_id: persona.into(),
            workspace_id: String::new(),
            branch: branch.into(),
            pr_number: number,
            url: Some("https://github.com/acme/app/pull/12".into()),
            opened_at: "2026-09-01 08:00:00".into(),
        }
    }

    #[test]
    fn opening_a_pull_request_is_recognised_however_it_is_typed() {
        for cmd in [
            "gh pr create",
            "gh pr create --fill --base main",
            "gh  pr   create",
            "cd /srv/app && gh pr create --title x",
            "sudo gh pr create",
            "GH_TOKEN=abc gh pr create",
            "gh --repo acme/app pr create",
            "/usr/bin/gh pr create",
            "glab mr create --fill",
            "bash -c 'gh pr create --fill'",
        ] {
            assert!(is_pr_create(cmd), "should be a PR create: {cmd}");
        }
    }

    #[test]
    fn checking_on_a_pull_request_is_not_opening_one() {
        // The agent has to be able to look at the PR that is blocking it, or the
        // refusal names something it cannot go and find.
        for cmd in [
            "gh pr list",
            "gh pr list --state open --json number,title",
            "gh pr view 12",
            "gh pr status",
            "gh pr merge 12 --squash",
            "gh pr close 12",
            "gh issue create --title x",
            "glab mr list",
            "git push -u origin wip/ada/fix",
            "echo 'run gh pr create when ready'",
            "grep -rn 'gh pr create' docs/",
        ] {
            assert!(!is_pr_create(cmd), "should not be a PR create: {cmd}");
        }
    }

    #[test]
    fn a_second_pull_request_is_refused_and_the_refusal_says_how_to_clear_it() {
        let open = vec![pr("ada", "wip/ada/fix-login", Some(12))];
        let msg = blocks(&open, "ada", "run_command", &json!({"command": "gh pr create --fill"}))
            .expect("a second PR must be refused");
        // The three things the model needs in order to act rather than retry: which PR,
        // which branch, and the one command that unblocks it.
        assert!(msg.contains("#12"), "{msg}");
        assert!(msg.contains("wip/ada/fix-login"), "{msg}");
        assert!(msg.contains("gh pr merge 12"), "{msg}");
        assert!(msg.contains("gh pr close 12"), "{msg}");
    }

    #[test]
    fn a_pull_request_without_a_number_still_names_a_next_step() {
        let open = vec![OpenPr {
            pr_number: None,
            url: None,
            ..pr("ada", "wip/ada/dns", None)
        }];
        let msg = blocks(&open, "ada", "run_command", &json!({"command": "gh pr create"}))
            .expect("still blocked");
        assert!(msg.contains("wip/ada/dns"), "{msg}");
        assert!(msg.contains("gh pr list --head wip/ada/dns"), "{msg}");
    }

    #[test]
    fn another_agents_open_pull_request_does_not_block_this_one() {
        // Keyed on persona, like every other agent-scoped table: two agents working on
        // two projects are not competing for one slot.
        let open = vec![pr("bruno", "wip/bruno/dns", Some(9))];
        assert!(blocks(&open, "ada", "run_command", &json!({"command": "gh pr create"})).is_none());
    }

    #[test]
    fn nothing_is_blocked_when_the_agent_has_no_pull_request_open() {
        assert!(blocks(&[], "ada", "run_command", &json!({"command": "gh pr create"})).is_none());
    }

    #[test]
    fn the_user_driving_the_console_by_hand_is_never_blocked() {
        // No persona means the person is typing. The rule exists to stop unattended
        // agents stacking branches, not to refuse the owner of the machine.
        let open = vec![pr("ada", "wip/ada/fix-login", Some(12))];
        assert!(blocks(&open, "", "run_command", &json!({"command": "gh pr create"})).is_none());
    }

    #[test]
    fn only_the_tools_that_run_a_shell_are_guarded() {
        let open = vec![pr("ada", "wip/ada/fix-login", Some(12))];
        // A tool that cannot reach GitHub cannot open a PR, and pretending otherwise
        // would block a file write whose content happens to mention the command.
        assert!(blocks(&open, "ada", "write_file", &json!({"command": "gh pr create"})).is_none());
        assert!(tool_can_open_pr("run_command"));
        assert!(tool_can_open_pr("local_run_command"));
        assert!(!tool_can_open_pr("read_file"));
    }

    #[test]
    fn a_command_line_is_split_the_way_a_shell_would_split_it() {
        // Quoted separators are text, not new commands — otherwise a commit message
        // containing `&&` invents a segment that was never run.
        assert_eq!(segments("a && b").len(), 2);
        assert_eq!(segments("echo 'a && b'").len(), 1);
        assert_eq!(segments("a; b | c").len(), 3);
    }
}

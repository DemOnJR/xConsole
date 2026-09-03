//! Project scoping for the local filesystem: which files a turn is allowed to touch.
//!
//! Servers have had this since the beginning — [`crate::ai::tools::resolve_target`]
//! refuses a `vps_id` outside the turn's selected targets, and every server-side tool
//! goes through it. The local side had nothing equivalent: `local_read_file` took
//! `args["path"]` and read it, so an agent hired for one project could read and rewrite
//! another project's `.env` without anything noticing. This is the missing half.
//!
//! The rule is the one [`crate::infra::projects::resolve_project_file`] already applies
//! to Terraform directories, generalised: resolve the path, and refuse it if it does not
//! land under the project's root. Resolution goes through `canonicalize`, so a `..`
//! chain and a symlink pointing out of the tree are the same case and both are refused.
//!
//! # What is deliberately not scoped
//!
//! A turn with no project (`ctx.workspace_id == None`) is unrestricted. That is not an
//! oversight: the agent the user talks to over WhatsApp runs exactly like that, and it
//! is the one that has to be able to look at anything in order to decide who should be
//! working on it. Scoping is what an agent gets when it is given a project to work on.
//!
//! A project whose directory has never been set is also unrestricted, because there is
//! no root to compare against. Setting the project directory is what turns the scope on,
//! and the refusal text says so.

use std::path::{Component, Path, PathBuf};

use crate::ai::tools::ToolContext;
use crate::storage::models::Persona;
use crate::storage::Db;

/// Tools a scoped agent may always call, whatever its `allowed_tools` says.
///
/// An agent restricted to `["local_read_file"]` that could not report what it read, ask
/// the one question that unblocks it, or keep its own board would be restricted into
/// uselessness — and the user would widen the list back out to everything, which is the
/// outcome this is trying to avoid. Every one of these either talks to a person or
/// writes to the agent's own bookkeeping; none of them reaches a file or a server.
const ALWAYS_ALLOWED: &[&str] = &[
    "agent_report",
    "agent_inbox",
    "agent_list",
    "agent_org",
    "ask_user",
    "present_plan",
    "todo_write",
    "feature_propose",
];

/// One project's boundary, as it applies to this turn.
#[derive(Debug, Clone)]
pub struct Scope {
    /// What the user calls the project. Every refusal names it, so the agent can say
    /// which project it was stopped by rather than "permission denied".
    pub project: String,
    /// The canonical project root. Everything the turn touches must be under it.
    pub root: PathBuf,
    /// The persona's own narrower list, relative to the root. Empty = the whole root.
    pub allowed_paths: Vec<String>,
}

/// Where a project's code lives on this PC, and what the project is called.
///
/// `None` when the workspace is gone, when it has no project directory set, or when its
/// code lives on a server — a VPS path is not a local path, and treating it as one would
/// scope local reads to a directory that does not exist here.
pub fn project_root(db: &Db, workspace_id: &str) -> Option<(String, PathBuf)> {
    let ws = db.get_workspace(workspace_id).ok().flatten()?;
    let loc: crate::ai::workspace_context::ProjectLocation =
        serde_json::from_str(ws.project_json.as_deref()?).ok()?;
    if loc.kind == "vps" {
        return None;
    }
    let path = loc.path.as_deref().map(str::trim).filter(|p| !p.is_empty())?;
    let root = PathBuf::from(path);
    // A root that does not exist yet cannot be canonicalised; normalise it instead so
    // the comparison is still made against something absolute.
    let root = root.canonicalize().unwrap_or_else(|_| normalize(&root));
    Some((ws.name, root))
}

/// The boundary this turn works inside, or `None` when the turn is unscoped.
pub fn scope_for(ctx: &ToolContext) -> Option<Scope> {
    let ws = ctx.workspace_id.as_deref().filter(|s| !s.is_empty())?;
    let (project, root) = project_root(&ctx.db, ws)?;
    let allowed_paths = crate::ai::persona_tools::current_persona(ctx)
        .map(|p| p.allowed_paths)
        .unwrap_or_default();
    Some(Scope {
        project,
        root,
        allowed_paths,
    })
}

/// What a tool is about to do with a path.
///
/// The project root binds both: an agent on one project has no business reading another
/// project's secrets either. The persona's own narrower `allowed_paths` binds writes
/// only, which is what makes "reads the whole codebase, writes only the tests" a thing
/// somebody can actually configure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Read,
    Write,
}

/// Resolve a path a tool is about to read, refusing anything outside the project.
///
/// Relative paths are resolved against the project root rather than against xConsole's
/// own working directory, which is the other half of the same bug: `local_grep_search`
/// with `path: "."` used to search whatever directory the app happened to be started in.
pub fn resolve_path(ctx: &ToolContext, path: &str) -> Result<PathBuf, String> {
    resolve_for(ctx, path, Intent::Read)
}

/// Resolve a path a tool is about to write to.
pub fn resolve_write_path(ctx: &ToolContext, path: &str) -> Result<PathBuf, String> {
    resolve_for(ctx, path, Intent::Write)
}

fn resolve_for(ctx: &ToolContext, path: &str, intent: Intent) -> Result<PathBuf, String> {
    resolve_in(scope_for(ctx).as_ref(), path, intent)
}

/// The decision itself, with the scope already looked up.
///
/// Split from `resolve_for` so the unscoped case is testable: a `ToolContext` needs a
/// live Tauri handle, and "no project means no restriction" is the rule most worth
/// having a test on, because it is the one that would quietly disable everything else.
fn resolve_in(scope: Option<&Scope>, path: &str, intent: Intent) -> Result<PathBuf, String> {
    match scope {
        Some(scope) => scope.resolve(path, intent),
        // Unscoped: the path is the path. Still normalised, so callers downstream can
        // treat every resolved path the same way.
        None => Ok(normalize(Path::new(path))),
    }
}

/// Whether this persona may call this tool at all.
///
/// An empty `allowed_tools` means every tool, which is what every persona created
/// before this existed has. A name may end in `*` to allow a family (`local_*`).
pub fn tool_allowed(persona: &Persona, tool: &str) -> bool {
    if persona.allowed_tools.is_empty() {
        return true;
    }
    if ALWAYS_ALLOWED.contains(&tool) || tool.starts_with("goal_") {
        return true;
    }
    persona.allowed_tools.iter().any(|allowed| {
        let allowed = allowed.trim();
        match allowed.strip_suffix('*') {
            Some(prefix) => tool.len() >= prefix.len() && tool[..prefix.len()].eq_ignore_ascii_case(prefix),
            None => allowed.eq_ignore_ascii_case(tool),
        }
    })
}

/// The refusal an agent reads when it calls a tool it was not given.
pub fn tool_scope_error(persona: &Persona, tool: &str) -> String {
    format!(
        "error: '{tool}' is not one of the tools {} was given (allowed: {}). Ask whoever \
         manages you to widen it, or hand the step to somebody who has it.",
        persona.name,
        persona.allowed_tools.join(", ")
    )
}

impl Scope {
    /// Resolve one path inside this scope.
    pub fn resolve(&self, path: &str, intent: Intent) -> Result<PathBuf, String> {
        let raw = path.trim();
        if raw.is_empty() {
            return Err(self.error("(empty)"));
        }
        let joined = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.root.join(raw)
        };
        let resolved = canonical_or_parent(&joined);
        if !resolved.starts_with(&self.root) {
            return Err(self.error(raw));
        }
        if intent == Intent::Write && !self.allowed_paths.is_empty() {
            let rel = resolved
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            // The root itself is always readable — an agent that cannot list the
            // directory it works in cannot find the files it was given.
            if !rel.is_empty() && !self.allowed_paths.iter().any(|g| glob_match(g, &rel)) {
                return Err(format!(
                    "error: {raw} is inside {} but not one of the files you may change \
                     ({}). You can read it. Changing it is somebody else's job — say what \
                     needs doing rather than doing it.",
                    self.project,
                    self.allowed_paths.join(", ")
                ));
            }
        }
        Ok(resolved)
    }

    fn error(&self, raw: &str) -> String {
        format!(
            "error: {raw} is outside the project {} ({}). You work on {} only — another \
             project's files are not yours to read or change. If the work genuinely \
             belongs there, tell your manager rather than reaching across.",
            self.project,
            self.root.display(),
            self.project
        )
    }
}

/// Canonicalise `path`, or — when it does not exist yet — its nearest existing ancestor
/// with the missing tail appended.
///
/// A write creates the file, so refusing to resolve a path because it is not there yet
/// would refuse every new file. Resolving the parent is enough: a symlinked parent is
/// followed, and the tail cannot itself be a symlink because it does not exist.
fn canonical_or_parent(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(parent) = cur.parent().map(Path::to_path_buf) {
        let Some(name) = cur.file_name().map(|n| n.to_os_string()) else { break };
        tail.push(name);
        if let Ok(c) = parent.canonicalize() {
            let mut out = c;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        if parent.as_os_str().is_empty() {
            break;
        }
        cur = parent;
    }
    normalize(path)
}

/// Lexical `..`/`.` removal, for paths that cannot be canonicalised at all.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Match a path against one glob: `*` within a segment, `**` across segments, `?` for
/// one character. A pattern naming a directory (`src-tauri/`, or plain `src-tauri`)
/// matches everything under it, because that is what somebody means when they type it.
fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim().trim_start_matches("./").trim_start_matches('/');
    if pattern.is_empty() {
        return false;
    }
    let path = path.trim_start_matches('/');
    if glob_inner(pattern.as_bytes(), path.as_bytes()) {
        return true;
    }
    // "src/" or "src" as a whole-directory grant.
    let dir = pattern.trim_end_matches('/');
    !dir.is_empty()
        && !dir.contains('*')
        && (path == dir || path.starts_with(&format!("{dir}/")))
}

/// Recursive glob. `**` consumes any number of characters including `/`; a single `*`
/// stops at a separator, so `src/*` is one level the way every other tool spells it.
///
/// Recursive rather than the usual one-star-backtrack loop on purpose: with two kinds of
/// star, remembering only the most recent resume point loses the outer one, and
/// `**/*.test.ts` stops matching anything in a subdirectory. Patterns here are a handful
/// of bytes, so the branching costs nothing.
fn glob_inner(pat: &[u8], text: &[u8]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }
    if pat[0] == b'*' {
        let deep = pat.len() > 1 && pat[1] == b'*';
        let rest = if deep { &pat[2..] } else { &pat[1..] };
        // `**/x` also matches `x`: "anywhere" includes here.
        if deep && rest.first() == Some(&b'/') && glob_inner(&rest[1..], text) {
            return true;
        }
        let mut i = 0usize;
        loop {
            if glob_inner(rest, &text[i..]) {
                return true;
            }
            if i >= text.len() {
                return false;
            }
            if !deep && text[i] == b'/' {
                return false;
            }
            i += 1;
        }
    }
    if text.is_empty() {
        return false;
    }
    if pat[0] == b'?' || pat[0] == text[0] {
        return glob_inner(&pat[1..], &text[1..]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway project tree: `<tmp>/xconsole_scope_<tag>/{a,b}`.
    fn tree(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("xconsole_scope_{tag}"));
        let _ = std::fs::remove_dir_all(&base);
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(a.join("src")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(b.join(".env"), "SECRET=1").unwrap();
        (a.canonicalize().unwrap(), b.canonicalize().unwrap())
    }

    fn scope_of(root: &Path, allowed: &[&str]) -> Scope {
        Scope {
            project: "Project A".into(),
            root: root.to_path_buf(),
            allowed_paths: allowed.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_path_outside_the_project_root_is_refused() {
        let (a, b) = tree("outside");
        let scope = scope_of(&a, &[]);
        let err = scope.resolve(&b.join(".env").to_string_lossy(), Intent::Read).unwrap_err();
        // The refusal has to name the project, or the agent cannot say what stopped it.
        assert!(err.contains("Project A"), "{err}");
        assert!(scope.resolve("src/main.rs", Intent::Read).is_ok());
    }

    #[test]
    fn dot_dot_traversal_is_refused() {
        let (a, _b) = tree("traversal");
        let scope = scope_of(&a, &[]);
        assert!(scope.resolve("../b/.env", Intent::Read).is_err());
        assert!(scope.resolve("src/../../b/.env", Intent::Read).is_err());
        // Traversal that stays inside is fine — it is a path, not an attack.
        assert!(scope.resolve("src/../src/main.rs", Intent::Read).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_pointing_out_of_the_project_is_refused() {
        let (a, b) = tree("symlink");
        std::os::unix::fs::symlink(&b, a.join("escape")).unwrap();
        let scope = scope_of(&a, &[]);
        let err = scope.resolve("escape/.env", Intent::Read).unwrap_err();
        assert!(err.contains("Project A"), "{err}");
    }

    #[test]
    fn a_write_to_a_file_that_does_not_exist_yet_is_allowed() {
        let (a, _b) = tree("newfile");
        let scope = scope_of(&a, &[]);
        let resolved = scope
            .resolve("src/brand_new/deep.rs", Intent::Write)
            .expect("new file inside the root");
        assert!(resolved.starts_with(&a), "{}", resolved.display());
        // But only inside. A new file outside is still outside.
        assert!(scope.resolve("../elsewhere/new.rs", Intent::Write).is_err());
    }

    #[test]
    fn an_unscoped_turn_allows_everything() {
        // No project means no scope, and no scope means the path is taken as given.
        // This is what makes the agent the user talks to over chat able to look
        // anywhere, which is what makes it the one that can decide who should be
        // working on what. Deliberate, and the reason scoping is safe to fail closed
        // everywhere else.
        let (a, b) = tree("unscoped");
        let outside = b.join(".env");
        for intent in [Intent::Read, Intent::Write] {
            assert!(resolve_in(None, &outside.to_string_lossy(), intent).is_ok());
            assert!(resolve_in(None, "/etc/hosts", intent).is_ok());
        }
        // The same path, with a scope, is refused. Same input, different turn.
        let scope = scope_of(&a, &[]);
        assert!(resolve_in(Some(&scope), &outside.to_string_lossy(), Intent::Read).is_err());
    }

    #[test]
    fn a_persona_writes_only_where_it_was_given_but_reads_the_project() {
        let (a, _b) = tree("allowed");
        let scope = scope_of(&a, &["src/**"]);
        assert!(scope.resolve("src/main.rs", Intent::Write).is_ok());
        let err = scope.resolve("Cargo.toml", Intent::Write).unwrap_err();
        assert!(err.contains("not one of the files you may change"), "{err}");
        // Reading it is fine. An engineer that cannot read the file it is not allowed to
        // change cannot understand what it is changing.
        assert!(scope.resolve("Cargo.toml", Intent::Read).is_ok());
        // A bare directory name is a grant of everything under it.
        let scope = scope_of(&a, &["src"]);
        assert!(scope.resolve("src/main.rs", Intent::Write).is_ok());
        assert!(scope.resolve("src/deep/nested/x.rs", Intent::Write).is_ok());
        assert!(scope.resolve("README.md", Intent::Write).is_err());
    }

    #[test]
    fn globs_match_the_way_people_write_them() {
        assert!(glob_match("src/**", "src/a/b.rs"));
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("src/*.rs", "src/a/main.rs"));
        assert!(glob_match("**/*.test.ts", "src/x/y.test.ts"));
        assert!(glob_match("**/*.test.ts", "y.test.ts"));
        assert!(glob_match("docs/", "docs/adr/1.md"));
        assert!(!glob_match("docs/", "src/docs.md"));
        assert!(glob_match("*.md", "README.md"));
        assert!(!glob_match("*.md", "docs/README.md"));
    }

    fn persona_with(tools: &[&str]) -> Persona {
        Persona {
            id: "p1".into(),
            name: "Ada".into(),
            role: String::new(),
            instructions: String::new(),
            targets: vec![],
            safety_mode: None,
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: None,
            workspace_id: None,
            allowed_paths: Vec::new(),
            allowed_tools: tools.iter().map(|s| s.to_string()).collect(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn an_empty_tool_list_means_every_tool() {
        let p = persona_with(&[]);
        assert!(tool_allowed(&p, "local_write_file"));
        assert!(tool_allowed(&p, "cloudflare_dns_create"));
    }

    #[test]
    fn a_restricted_agent_keeps_the_tools_it_needs_to_speak() {
        let p = persona_with(&["local_read_file", "local_*"]);
        assert!(tool_allowed(&p, "local_read_file"));
        assert!(tool_allowed(&p, "local_grep_search"));
        assert!(!tool_allowed(&p, "cloudflare_dns_create"));
        // Reporting and asking are never taken away, or a scoped agent goes silent.
        assert!(tool_allowed(&p, "agent_report"));
        assert!(tool_allowed(&p, "ask_user"));
        assert!(tool_allowed(&p, "goal_add_task"));
    }
}

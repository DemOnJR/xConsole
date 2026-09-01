//! Per-session file freshness: remember mtime after a read so a later write
//! can refuse to clobber a file the agent has not re-read (optimistic concurrency
//! protection). Keys are session + vps + path.

use std::sync::OnceLock;

use dashmap::DashMap;

struct Stamp {
    session: String,
    mtime: String,
    encoding: Option<String>,
}

fn map() -> &'static DashMap<String, Stamp> {
    static MAP: OnceLock<DashMap<String, Stamp>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

fn key(vps_id: &str, path: &str) -> String {
    format!("{vps_id}\0{path}")
}

/// The scope standing in for the user's own machine. A local path is tracked
/// under this instead of a VPS id, so a write to this PC is guarded exactly
/// like a write to a server rather than going unchecked. The \x01 prefix
/// cannot collide with a real vps_id, which is always a UUID.
pub const LOCAL: &str = "\u{1}local";

/// The on-disk mtime of a local path in the same shape `stat -c %Y` gives for a
/// server, or empty when the file does not exist yet (a fresh write is never a
/// clobber). Both sides of the check therefore speak one language.
pub fn local_mtime(path: &str) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        // Nanoseconds, not the whole seconds `stat -c %Y` reports: locally we
        // can see two writes inside the same second, and that is exactly the
        // race worth catching. The value is only ever compared for equality,
        // so the extra precision costs nothing.
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
}

/// Record that this session just read `path` at `mtime` (unix seconds or raw
/// `stat` output). Empty mtime is ignored.
#[allow(dead_code)]
pub fn note_read(session_id: &str, vps_id: &str, path: &str, mtime: &str) {
    note_read_with_encoding(session_id, vps_id, path, mtime, None);
}

/// Record that this session just read `path` at `mtime` with a detected charset encoding.
pub fn note_read_with_encoding(
    session_id: &str,
    vps_id: &str,
    path: &str,
    mtime: &str,
    encoding: Option<&str>,
) {
    let mtime = mtime.trim();
    if mtime.is_empty() {
        return;
    }
    map().insert(
        key(vps_id, path),
        Stamp {
            session: session_id.to_string(),
            mtime: mtime.to_string(),
            encoding: encoding.map(str::to_string),
        },
    );
}

/// Re-stamp `path` after this session wrote it, keeping the charset the read
/// detected.
///
/// Without this a write left the stamp pointing at the mtime from before it, so
/// the *next* write to the same file compared an old stamp against a file this
/// session had just changed and refused it as somebody else's edit. Two edits
/// to one file in a row were impossible, and the warning — the one that exists
/// to stop real work being lost — became a thing to be worked around.
pub fn note_write(session_id: &str, vps_id: &str, path: &str, mtime: &str) {
    let mtime = mtime.trim();
    if mtime.is_empty() {
        return;
    }
    let encoding = get_encoding(session_id, vps_id, path);
    map().insert(
        key(vps_id, path),
        Stamp {
            session: session_id.to_string(),
            mtime: mtime.to_string(),
            encoding,
        },
    );
}

/// Get the detected encoding from a previous read in this session.
pub fn get_encoding(session_id: &str, vps_id: &str, path: &str) -> Option<String> {
    let stamp = map().get(&key(vps_id, path))?;
    if stamp.session == session_id {
        stamp.encoding.clone()
    } else {
        None
    }
}

/// If this session already read `path` and the on-disk mtime has changed,
/// return an error the agent must resolve by calling `read_file` again.
pub fn check_write(session_id: &str, vps_id: &str, path: &str, current_mtime: &str) -> Result<(), String> {
    let current = current_mtime.trim();
    if current.is_empty() {
        return Ok(());
    }
    let Some(stamp) = map().get(&key(vps_id, path)) else {
        return Ok(());
    };
    if stamp.session != session_id {
        return Ok(());
    }
    if stamp.mtime == current {
        return Ok(());
    }
    // Name the tool that actually re-reads this scope. Telling an agent to call
    // `read_file` for a path on the user's own PC sends it to the wrong machine.
    let reread = if vps_id == LOCAL { "local_read_file" } else { "read_file" };
    Err(format!(
        "error: {path} changed on disk since you last read it. Something else wrote to it \
         — another agent, a git pull, or the user. Your copy is stale and writing it now \
         would silently throw their change away. Call {reread} on this path again, redo \
         your edit against what it returns, then write."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_allowed_when_unread() {
        assert!(check_write("s", "v", "/tmp/new", "1").is_ok());
    }

    #[test]
    fn a_second_write_is_not_mistaken_for_someone_elses_change() {
        // Read, write, write again. The second write must go through: the only
        // thing that moved the mtime was our own first write. Before note_write
        // existed this failed, and the agent was told another process had
        // touched the file — so consecutive edits to one file were impossible
        // and the warning read as noise.
        note_read("s", "v", "/tmp/twice", "100");
        assert!(check_write("s", "v", "/tmp/twice", "100").is_ok());
        note_write("s", "v", "/tmp/twice", "200");
        assert!(
            check_write("s", "v", "/tmp/twice", "200").is_ok(),
            "our own write was reported as somebody else's"
        );
    }

    #[test]
    fn a_write_keeps_the_charset_the_read_detected() {
        note_read_with_encoding("s", "v", "/tmp/enc", "100", Some("windows-1252"));
        note_write("s", "v", "/tmp/enc", "200");
        assert_eq!(
            get_encoding("s", "v", "/tmp/enc").as_deref(),
            Some("windows-1252"),
            "re-stamping a write forgot the encoding, so the next write would \
             rewrite the file as utf-8 and mangle every accented character"
        );
    }

    #[test]
    fn write_blocked_when_mtime_moved() {
        note_read("s", "v", "/tmp/x", "100");
        let err = check_write("s", "v", "/tmp/x", "200").unwrap_err();
        assert!(err.contains("changed on disk"), "{err}");
        assert!(check_write("s", "v", "/tmp/x", "100").is_ok());
    }
}

//! Per-session file freshness: remember mtime after a read so a later write
//! can refuse to clobber a file the agent has not re-read (Claude/claw-code
//! style). Keys are session + vps + path.

use std::sync::OnceLock;

use dashmap::DashMap;

struct Stamp {
    session: String,
    mtime: String,
}

fn map() -> &'static DashMap<String, Stamp> {
    static MAP: OnceLock<DashMap<String, Stamp>> = OnceLock::new();
    MAP.get_or_init(DashMap::new)
}

fn key(vps_id: &str, path: &str) -> String {
    format!("{vps_id}\0{path}")
}

/// Record that this session just read `path` at `mtime` (unix seconds or raw
/// `stat` output). Empty mtime is ignored.
pub fn note_read(session_id: &str, vps_id: &str, path: &str, mtime: &str) {
    let mtime = mtime.trim();
    if mtime.is_empty() {
        return;
    }
    map().insert(
        key(vps_id, path),
        Stamp {
            session: session_id.to_string(),
            mtime: mtime.to_string(),
        },
    );
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
    Err(format!(
        "error: file changed on disk since you last read it (mtime was {}, now {current}). \
         Call read_file on this path again, then retry the write.",
        stamp.mtime
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
    fn write_blocked_when_mtime_moved() {
        note_read("s", "v", "/tmp/x", "100");
        let err = check_write("s", "v", "/tmp/x", "200").unwrap_err();
        assert!(err.contains("changed on disk"), "{err}");
        assert!(check_write("s", "v", "/tmp/x", "100").is_ok());
    }
}

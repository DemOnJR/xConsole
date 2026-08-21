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

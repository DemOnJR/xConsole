//! Shared file-edit / slice / line-number helpers for remote and local tools.
//! This is how Claude stays cheap on large files: grep → line → read a window
//! → replace a unique snippet. Never rewrite the whole file.

/// Replace `old` with `new`. `old` must be unique unless `replace_all`.
pub fn apply_edit(src: &str, old: &str, new: &str, replace_all: bool) -> Result<(String, usize), String> {
    if old.is_empty() {
        return Err("error: old_string is empty".into());
    }
    let count = src.matches(old).count();
    if count == 0 {
        return Err(
            "error: old_string not found in the file. Call read_file (or grep_search) again \
             and copy the exact current text."
                .into(),
        );
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "error: old_string matched {count} times. Pass a larger unique snippet, \
             or set replace_all=true."
        ));
    }
    let n = if replace_all { count } else { 1 };
    let next = if replace_all {
        src.replace(old, new)
    } else {
        src.replacen(old, new, 1)
    };
    Ok((next, n))
}

/// 1-based window. `offset` defaults to 1, `limit` of 0 means "rest of file".
pub fn slice_lines(src: &str, offset: Option<u32>, limit: Option<u32>) -> (String, u32, u32, u32) {
    let lines: Vec<&str> = src.lines().collect();
    let total = lines.len() as u32;
    let start = offset.unwrap_or(1).max(1);
    let start_idx = (start as usize).saturating_sub(1).min(lines.len());
    let take = match limit {
        Some(n) if n > 0 => n as usize,
        _ => lines.len().saturating_sub(start_idx),
    };
    let window = &lines[start_idx..start_idx.saturating_add(take).min(lines.len())];
    let body = number_lines(window, start);
    (body, start, window.len() as u32, total)
}

pub fn number_lines(lines: &[&str], start: u32) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>6}|{}", start as usize + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// If the caller did not ask for a window and the file is huge, keep a head
/// and tell them to grep / page. 250 lines is ~enough for a unit and cheap.
pub const DEFAULT_READ_CAP: u32 = 250;

pub fn format_read(src: &str, offset: Option<u32>, limit: Option<u32>) -> String {
    let explicit = offset.is_some() || limit.is_some();
    let limit = if explicit {
        limit
    } else if src.lines().count() as u32 > DEFAULT_READ_CAP {
        Some(DEFAULT_READ_CAP)
    } else {
        None
    };
    let (body, start, shown, total) = slice_lines(src, offset.or(Some(1)), limit);
    if shown < total {
        format!(
            "[lines {start}–{} of {total} — use offset/limit or grep_search for the rest]\n{body}",
            start + shown - 1
        )
    } else {
        format!("[lines 1–{total}]\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_replace() {
        let (out, n) = apply_edit("a\nb\nc\n", "b", "B", false).unwrap();
        assert_eq!(n, 1);
        assert_eq!(out, "a\nB\nc\n");
    }

    #[test]
    fn refuses_ambiguous_replace() {
        let err = apply_edit("x x x", "x", "y", false).unwrap_err();
        assert!(err.contains("3 times"), "{err}");
    }

    #[test]
    fn replace_all() {
        let (out, n) = apply_edit("x x x", "x", "y", true).unwrap();
        assert_eq!(n, 3);
        assert_eq!(out, "y y y");
    }

    #[test]
    fn numbers_and_slices() {
        let src = "a\nb\nc\nd\n";
        let (body, start, shown, total) = slice_lines(src, Some(2), Some(2));
        assert_eq!(start, 2);
        assert_eq!(shown, 2);
        assert_eq!(total, 4);
        assert!(body.contains("     2|b"));
        assert!(body.contains("     3|c"));
        assert!(!body.contains("|d"));
    }
}

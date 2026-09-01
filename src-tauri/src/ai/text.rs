//! Small shared text helpers: UTF-8-safe truncation and token estimates.
//! Centralized so the many "slice a string for the context window" call sites
//! cannot panic on multibyte input (MOTD box-drawing, accented paths, etc.).

/// Largest index `<= idx` that lies on a UTF-8 char boundary. Stable-Rust
/// stand-in for the still-unstable `str::floor_char_boundary`.
pub fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Borrow at most `max_bytes` of `s` without ever splitting a codepoint.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    &s[..floor_char_boundary(s, max_bytes)]
}

/// Keep the **last** `max_bytes` of an append-ordered document, on a line boundary.
///
/// `MEMORY.md` and `TASTE.md` grow by appending, so their newest entries — the ones
/// the agent just learned — live at the end. Truncating them from the front (the
/// obvious `&s[..max]`) meant that once a file passed its cap, every fact saved from
/// then on was invisible to the prompt: the agent kept writing to a file it could no
/// longer read back. Keeping the tail keeps what was learned most recently.
///
/// Cuts at a newline where possible so the block never opens mid-bullet, and marks
/// the elision so the model knows older entries exist rather than assuming this is
/// everything it has ever been told.
pub fn keep_newest(s: &str, max_bytes: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_bytes {
        return trimmed.to_string();
    }
    let start = trimmed.len() - max_bytes;
    let start = floor_char_boundary(trimmed, start);
    let tail = &trimmed[start..];
    // Prefer starting at the next line break so the first entry shown is whole.
    let tail = match tail.find('\n') {
        Some(nl) if nl + 1 < tail.len() => &tail[nl + 1..],
        _ => tail,
    };
    format!("…(older entries elided)\n{}", tail.trim_start())
}

/// Outcome of appending to a bullet document.
#[derive(Debug, PartialEq, Eq)]
pub enum BulletAppend {
    /// The entry had no non-empty lines — nothing was asked for.
    Empty,
    /// Every line was already present verbatim; the document is unchanged.
    Unchanged,
    /// The new document contents, ready to write.
    Updated(String),
}

/// Append `entry` to an append-ordered bullet document, one bullet per non-empty
/// line, skipping lines already present verbatim.
///
/// `MEMORY.md` and `TASTE.md` are the same shape and had byte-identical copies of
/// this logic (plus a third copy of the marker stripping), so a fix to one silently
/// left the other behind. Pure in/out, so the dedup rules are testable without
/// touching the filesystem.
pub fn append_bullets(existing: &str, entry: &str) -> BulletAppend {
    let lines: Vec<&str> = entry
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(strip_single_marker)
        .collect();
    if lines.is_empty() {
        return BulletAppend::Empty;
    }
    let mut content = existing.to_string();
    let mut added = false;
    for line in lines {
        // Compare stripped-to-stripped. The old check tested the stripped new line
        // against the raw existing line, but every line this function writes is
        // prefixed "- " — so "alpha" never matched the "- alpha" it had just added,
        // and saving the same fact twice appended it twice. Memory filled up with
        // repeats of whatever the agent re-learned most often.
        if content
            .lines()
            .any(|l| strip_single_marker(l.trim()) == line)
        {
            continue; // dedup exact lines only
        }
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("- ");
        content.push_str(line);
        content.push('\n');
        added = true;
    }
    if added {
        BulletAppend::Updated(content)
    } else {
        BulletAppend::Unchanged
    }
}

/// Strip a single leading bullet marker + space (never a `--flag`'s dashes).
pub fn strip_single_marker(line: &str) -> &str {
    let l = line.trim_start();
    for m in ["- ", "* ", "• ", "-", "*", "•"] {
        if let Some(rest) = l.strip_prefix(m) {
            return rest.trim_start();
        }
    }
    l
}

/// Rough token estimate using the common ~4-characters-per-token heuristic.
/// Kept for hot-path budget math (trimming large snapshots) where exactness
/// doesn't matter; user-facing counts use [`count_tokens`].
pub fn estimate_tokens_from_len(char_len: usize) -> usize {
    (char_len + 3) / 4
}

/// Accurate token count via a BPE tokenizer (cl100k_base), used for the
/// user-facing context/token calculator so counts are correct for every
/// provider — including CLI tools (Cursor/Codex/OpenCode) that report no usage.
/// Falls back to the ~4-chars heuristic if the tokenizer can't initialize.
pub fn count_tokens(text: &str) -> usize {
    use std::sync::OnceLock;
    use tiktoken_rs::CoreBPE;
    static BPE: OnceLock<Option<CoreBPE>> = OnceLock::new();
    if text.is_empty() {
        return 0;
    }
    match BPE.get_or_init(|| tiktoken_rs::cl100k_base().ok()) {
        Some(bpe) => bpe.encode_ordinary(text).len(),
        None => estimate_tokens_from_len(text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_splits_a_codepoint() {
        let s = "héllo wörld"; // multibyte é / ö
        for i in 0..=s.len() {
            // Slicing at the clamped boundary must not panic.
            let _ = &s[..floor_char_boundary(s, i)];
        }
        assert_eq!(truncate_bytes("aé", 2), "a"); // byte 2 is mid-é → back off to 1
    }

    #[test]
    fn keep_newest_returns_short_input_untouched() {
        assert_eq!(keep_newest("  - one\n- two  ", 100), "- one\n- two");
        assert_eq!(keep_newest("", 100), "");
    }

    #[test]
    fn keep_newest_keeps_the_end_not_the_start() {
        let doc = (0..200)
            .map(|i| format!("- fact {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = keep_newest(&doc, 200);
        // The most recently appended entry is what a memory file exists to surface.
        assert!(out.contains("- fact 199"), "{out}");
        assert!(!out.contains("- fact 0\n"), "{out}");
        assert!(out.starts_with("…(older entries elided)"), "{out}");
    }

    #[test]
    fn keep_newest_starts_on_a_whole_line() {
        let doc = "- alpha\n- bravo\n- charlie";
        let out = keep_newest(&doc, 12);
        let first_entry = out.lines().nth(1).unwrap();
        assert!(first_entry.starts_with("- "), "{out}");
    }

    #[test]
    fn keep_newest_never_splits_a_codepoint() {
        // A cut landing mid-é must back off rather than panic.
        let doc = "aéaéaéaéaé";
        for max in 1..doc.len() {
            let _ = keep_newest(doc, max);
        }
    }

    #[test]
    fn append_bullets_normalises_markers_and_adds_one_per_line() {
        let out = append_bullets("", "* first\n\n- second\nthird");
        assert_eq!(
            out,
            BulletAppend::Updated("- first\n- second\n- third\n".into())
        );
    }

    #[test]
    fn append_bullets_never_eats_a_flags_dashes() {
        let out = append_bullets("", "--force is required here");
        assert_eq!(
            out,
            BulletAppend::Updated("- -force is required here\n".into())
        );
    }

    #[test]
    fn append_bullets_skips_lines_already_present() {
        let existing = "- alpha\n";
        // Written with a marker, offered without one: still the same fact.
        assert_eq!(append_bullets(existing, "alpha"), BulletAppend::Unchanged);
        assert_eq!(append_bullets(existing, "- alpha"), BulletAppend::Unchanged);
        assert_eq!(
            append_bullets(existing, "alpha\nbravo"),
            BulletAppend::Updated("- alpha\n- bravo\n".into())
        );
    }

    #[test]
    fn append_bullets_is_idempotent_across_calls() {
        // Saving the same fact repeatedly must not grow the document — duplicates
        // used to accumulate and crowd out everything else in the prompt budget.
        let mut doc = String::new();
        for _ in 0..5 {
            if let BulletAppend::Updated(next) = append_bullets(&doc, "prefers k3s") {
                doc = next;
            }
        }
        assert_eq!(doc, "- prefers k3s\n");
    }

    #[test]
    fn append_bullets_reports_an_empty_entry() {
        assert_eq!(append_bullets("- a\n", "   \n\n"), BulletAppend::Empty);
    }

    #[test]
    fn append_bullets_repairs_a_document_with_no_trailing_newline() {
        assert_eq!(
            append_bullets("- alpha", "bravo"),
            BulletAppend::Updated("- alpha\n- bravo\n".into())
        );
    }

    #[test]
    fn token_estimate_rounds_up() {
        assert_eq!(estimate_tokens_from_len(0), 0);
        assert_eq!(estimate_tokens_from_len(1), 1);
        assert_eq!(estimate_tokens_from_len(4), 1);
        assert_eq!(estimate_tokens_from_len(5), 2);
    }
}

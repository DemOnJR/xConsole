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
    fn token_estimate_rounds_up() {
        assert_eq!(estimate_tokens_from_len(0), 0);
        assert_eq!(estimate_tokens_from_len(1), 1);
        assert_eq!(estimate_tokens_from_len(4), 1);
        assert_eq!(estimate_tokens_from_len(5), 2);
    }
}

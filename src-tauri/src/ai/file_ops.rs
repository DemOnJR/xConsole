//! Shared file-edit / slice / line-number helpers for remote and local tools.
//! This is how Claude stays cheap on large files: grep → line → read a window
//! → replace a unique snippet. Never rewrite the whole file.

/// Replace `old` with `new`. `old` must be unique unless `replace_all`.
/// One replacement to make in a file.
pub struct Edit<'a> {
    pub old: &'a str,
    pub new: &'a str,
    pub replace_all: bool,
}

/// Why an `old_string` did not match, when something very close to it is there.
///
/// "Not found, read the file again" is true and nearly useless: the agent copied that
/// text from a read a moment ago, so it re-reads, sees the same thing, and tries again.
/// Almost always the difference is whitespace — tabs against spaces, an indent that
/// changed, a trailing space — which is invisible in a diff and in the model's own
/// output. Naming the line and showing what is actually there ends the loop in one step
/// instead of three.
fn near_miss(src: &str, old: &str) -> Option<String> {
    let needle = old.lines().next()?.trim();
    if needle.len() < 4 {
        return None;
    }
    let (line_no, actual) = src
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim() == needle)
        .map(|(i, l)| (i + 1, l))?;
    let want = old.lines().next().unwrap_or("");
    let why = if want.trim_start() != want || actual.trim_start() != actual {
        let w = want.len() - want.trim_start().len();
        let a = actual.len() - actual.trim_start().len();
        if w != a {
            format!("the indentation differs ({w} leading characters in yours, {a} in the file)")
        } else {
            "the leading whitespace differs (tabs against spaces?)".to_string()
        }
    } else if want.trim_end() != want || actual.trim_end() != actual {
        "there is trailing whitespace on one of them".to_string()
    } else {
        "it differs somewhere in the whitespace".to_string()
    };
    Some(format!(
        " Line {line_no} is the same text, but {why}. The file has:\n{actual:?}\nyou sent:\n{want:?}"
    ))
}

/// Apply several edits in one pass, or none at all.
///
/// Each is applied to the result of the last, so an edit may touch text an earlier one
/// produced. Atomic on purpose: a batch that half-applied would leave the file in a
/// state neither the agent nor the user expects, and the agent would then read it back,
/// find its own half-finished work, and try to reconcile it. Better to change nothing
/// and say which edit was wrong.
///
/// The point of the batch is not tidiness. Every edit over SSH reads the whole file and
/// writes the whole file back; five edits to one file is five of each. This makes it one.
pub fn apply_edits(src: &str, edits: &[Edit<'_>]) -> Result<(String, usize), String> {
    if edits.is_empty() {
        return Err("error: no edits given".into());
    }
    let mut cur = src.to_string();
    let mut total = 0usize;
    for (i, e) in edits.iter().enumerate() {
        match apply_edit(&cur, e.old, e.new, e.replace_all) {
            Ok((next, n)) => {
                cur = next;
                total += n;
            }
            // Numbered, because "old_string not found" says nothing about which of five
            // it was.
            Err(err) => {
                return Err(format!(
                    "{err}\n\nThis was edit {} of {}; nothing was written — the file is \
                     unchanged.",
                    i + 1,
                    edits.len()
                ))
            }
        }
    }
    Ok((cur, total))
}

pub fn apply_edit(src: &str, old: &str, new: &str, replace_all: bool) -> Result<(String, usize), String> {
    if old.is_empty() {
        return Err("error: old_string is empty".into());
    }
    let count = src.matches(old).count();
    if count == 0 {
        return Err(format!(
            "error: old_string not found in the file.{} Call read_file again and copy the \
             exact current text.",
            near_miss(src, old).unwrap_or_default()
        ));
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
/// A window of numbered lines: (body, first line number, lines shown, total lines).
///
/// A negative offset counts back from the end, so -100 is the last hundred lines.
/// Tailing a log is most of what reading a file on a server is for, and without
/// this the only way to do it was `run_command tail`, which leaves the file
/// tooling — and with it the freshness stamp that stops the next write clobbering
/// someone else's change.
pub fn slice_lines(src: &str, offset: Option<i64>, limit: Option<u32>) -> (String, u32, u32, u32) {
    let lines: Vec<&str> = src.lines().collect();
    let total = lines.len() as u32;
    let start = match offset.unwrap_or(1) {
        n if n < 0 => (total as i64 + n + 1).max(1) as u32,
        n => (n as u32).max(1),
    };
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
        .map(|(i, line)| format!("{:>6}|{}", start as usize + i, clip_line(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The longest single line returned from a read.
///
/// The line cap is what actually protects the context, not the line *count*: a
/// minified bundle, a one-line JSON dump or a base64 blob is a single line
/// megabytes long, and a 250-line ceiling lets all of it through untouched. One
/// read of a `.min.js` could take out the whole window.
pub const MAX_LINE_LENGTH: usize = 2000;

/// One line, cut to something a reader can hold, saying what was cut.
///
/// Silently truncating would be worse than not truncating: an agent that cannot
/// tell a shortened line from a complete one will edit against the half it saw.
fn clip_line(line: &str) -> String {
    if line.len() <= MAX_LINE_LENGTH {
        return line.to_string();
    }
    let mut cut = MAX_LINE_LENGTH;
    while cut > 0 && !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}… [line truncated, {} more characters]",
        &line[..cut],
        line.chars().count().saturating_sub(line[..cut].chars().count())
    )
}

/// If the caller did not ask for a window and the file is huge, keep a head
/// and tell them to grep / page. 250 lines is ~enough for a unit and cheap.
pub const DEFAULT_READ_CAP: u32 = 250;

pub fn format_read(src: &str, offset: Option<i64>, limit: Option<u32>) -> String {
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

/// Detect character encoding from raw byte slice.
pub fn detect_encoding(bytes: &[u8]) -> &'static str {
    if bytes.is_empty() {
        return "utf-8";
    }
    // Check BOMs
    if bytes.len() >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf {
        return "utf-8-bom";
    }
    if bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] == 0xfe {
        return "utf-16le";
    }
    if bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff {
        return "utf-16be";
    }

    // Check valid UTF-8
    if std::str::from_utf8(bytes).is_ok() {
        return "utf-8";
    }

    // Heuristics
    let mut cyrillic_score = 0;
    let mut central_euro_score = 0;
    let mut korean_score = 0;

    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b >= 0xc0 {
            cyrillic_score += 1;
        }
        if matches!(b, 0xa3 | 0xa5 | 0xaf | 0xb3 | 0xb9 | 0xba | 0xde | 0xfe | 0xe3 | 0xe2 | 0xee) {
            central_euro_score += 1;
        }
        if (0xa1..=0xfe).contains(&b) && i + 1 < len {
            let next = bytes[i + 1];
            if (0xa1..=0xfe).contains(&next) {
                korean_score += 2;
                i += 1;
            }
        }
        i += 1;
    }

    if cyrillic_score as f32 / len as f32 > 0.05 && cyrillic_score > central_euro_score {
        return "windows-1251";
    }
    if korean_score as f32 / len as f32 > 0.08 {
        return "euc-kr";
    }
    if central_euro_score as f32 / len as f32 > 0.03 {
        return "windows-1250";
    }

    "windows-1252"
}

/// Decode raw bytes to Unicode string with the specified encoding.
#[allow(dead_code)]
pub fn decode_text_with_charset(bytes: &[u8], encoding: &str) -> String {
    let norm = encoding.to_lowercase();
    match norm.as_str() {
        "utf-8" => String::from_utf8_lossy(bytes).into_owned(),
        "utf-8-bom" => {
            let slice = if bytes.len() >= 3 && bytes[0] == 0xef && bytes[1] == 0xbb && bytes[2] == 0xbf {
                &bytes[3..]
            } else {
                bytes
            };
            String::from_utf8_lossy(slice).into_owned()
        }
        "utf-16le" => {
            let u16s: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u16s)
        }
        "utf-16be" => {
            let u16s: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u16s)
        }
        "windows-1251" => {
            // Decode Windows-1251 single-byte Cyrillic
            bytes
                .iter()
                .map(|&b| {
                    if b < 0x80 {
                        b as char
                    } else if (0xc0..=0xff).contains(&b) {
                        char::from_u32(0x0410 + (b - 0xc0) as u32).unwrap_or('?')
                    } else if b == 0xa8 {
                        'Ё'
                    } else if b == 0xb8 {
                        'ё'
                    } else if b == 0xb9 {
                        '№'
                    } else {
                        '?'
                    }
                })
                .collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Encode Unicode string into bytes of the specified encoding.
pub fn encode_text_with_charset(text: &str, encoding: &str) -> Vec<u8> {
    let norm = encoding.to_lowercase();
    match norm.as_str() {
        "utf-8" => text.as_bytes().to_vec(),
        "utf-8-bom" => {
            let mut out = vec![0xef, 0xbb, 0xbf];
            out.extend_from_slice(text.as_bytes());
            out
        }
        "utf-16le" => {
            let mut out = Vec::with_capacity(text.len() * 2);
            for c in text.encode_utf16() {
                out.extend_from_slice(&c.to_le_bytes());
            }
            out
        }
        "utf-16be" => {
            let mut out = Vec::with_capacity(text.len() * 2);
            for c in text.encode_utf16() {
                out.extend_from_slice(&c.to_be_bytes());
            }
            out
        }
        "windows-1251" => {
            text.chars()
                .map(|c| {
                    let u = c as u32;
                    if u < 0x80 {
                        u as u8
                    } else if (0x0410..=0x044f).contains(&u) {
                        (0xc0 + (u - 0x0410)) as u8
                    } else if c == 'Ё' {
                        0xa8
                    } else if c == 'ё' {
                        0xb8
                    } else if c == '№' {
                        0xb9
                    } else {
                        b'?'
                    }
                })
                .collect()
        }
        _ => text.as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_and_round_trip_encodings() {
        assert_eq!(detect_encoding(&[0xef, 0xbb, 0xbf, b'a']), "utf-8-bom");
        assert_eq!(detect_encoding(&[0xff, 0xfe, b'a', 0]), "utf-16le");
        assert_eq!(detect_encoding("hello world".as_bytes()), "utf-8");

        let cyrillic = "Привет";
        let encoded_1251 = encode_text_with_charset(cyrillic, "windows-1251");
        assert_eq!(detect_encoding(&encoded_1251), "windows-1251");
        let decoded = decode_text_with_charset(&encoded_1251, "windows-1251");
        assert_eq!(decoded, cyrillic);
    }

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

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn e<'a>(old: &'a str, new: &'a str) -> Edit<'a> {
        Edit { old, new, replace_all: false }
    }

    #[test]
    fn several_edits_apply_in_one_pass() {
        // The point is not tidiness: every edit over SSH reads the whole file and writes
        // the whole file back, so five edits to one file is five of each.
        let src = "one\ntwo\nthree\n";
        let (out, n) = apply_edits(src, &[e("one", "1"), e("three", "3")]).unwrap();
        assert_eq!(out, "1\ntwo\n3\n");
        assert_eq!(n, 2);
    }

    #[test]
    fn a_later_edit_can_touch_what_an_earlier_one_produced() {
        // Each applies to the result of the last, which is what makes a rename followed
        // by a fix-up work in one call.
        let (out, _) = apply_edits("alpha", &[e("alpha", "beta"), e("beta", "gamma")]).unwrap();
        assert_eq!(out, "gamma");
    }

    #[test]
    fn one_bad_edit_writes_nothing_and_says_which() {
        // A half-applied batch leaves the file in a state neither side expects, and the
        // agent then reads back its own unfinished work and tries to reconcile it.
        let src = "keep this\n";
        let err = apply_edits(src, &[e("keep", "kept"), e("missing", "x")]).unwrap_err();
        assert!(err.contains("edit 2 of 2"), "{err}");
        assert!(err.contains("unchanged"), "{err}");
    }

    #[test]
    fn a_whitespace_mismatch_is_named_rather_than_left_to_guess() {
        // "Not found, read it again" is true and useless: the agent copied that text
        // from a read a moment ago, re-reads, sees the same thing, and tries again. The
        // difference is almost always whitespace, which is invisible in its own output.
        let src = "fn main() {\n\tlet x = 1;\n}\n";
        let err = apply_edit(src, "    let x = 1;", "    let x = 2;", false).unwrap_err();
        assert!(err.contains("Line 2"), "{err}");
        assert!(err.contains("indentation") || err.contains("whitespace"), "{err}");
        // And it shows both, quoted, so the difference is actually visible.
        assert!(err.contains("\\t"), "the file's real text should be shown escaped: {err}");
    }

    #[test]
    fn a_genuinely_absent_string_does_not_invent_a_near_miss() {
        // A false explanation is worse than none: it sends the agent to fix whitespace
        // in a line that has nothing to do with what it asked for.
        let err = apply_edit("alpha\nbeta\n", "something else entirely", "x", false).unwrap_err();
        assert!(!err.contains("Line "), "{err}");
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn uniqueness_is_still_enforced_inside_a_batch() {
        // The safety property of a single edit must not be lost by batching them.
        let src = "x\nx\n";
        let err = apply_edits(src, &[e("x", "y")]).unwrap_err();
        assert!(err.contains("matched 2 times"), "{err}");
    }
}

#[cfg(test)]
mod read_window_tests {
    use super::*;

    fn doc(n: u32) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn a_negative_offset_reads_the_tail_of_a_log() {
        // Tailing is most of what reading a file on a server is for. Without this
        // the only way was `run_command tail`, which leaves the file tooling — and
        // with it the freshness stamp that stops the next write clobbering someone.
        let (body, start, shown, total) = slice_lines(&doc(500), Some(-3), None);
        assert_eq!((start, shown, total), (498, 3, 500));
        assert!(body.contains("line 500"), "{body}");
        assert!(!body.contains("line 497"), "{body}");
    }

    #[test]
    fn asking_for_more_tail_than_the_file_has_gives_the_whole_file() {
        let (body, start, shown, _) = slice_lines(&doc(4), Some(-99), None);
        assert_eq!((start, shown), (1, 4));
        assert!(body.contains("line 1") && body.contains("line 4"));
    }

    #[test]
    fn a_positive_offset_still_counts_from_the_top() {
        let (_, start, shown, _) = slice_lines(&doc(100), Some(10), Some(5));
        assert_eq!((start, shown), (10, 5));
        // 0 is not a line number; it must not wrap round to the end.
        let (_, start, _, _) = slice_lines(&doc(100), Some(0), Some(1));
        assert_eq!(start, 1);
    }

    #[test]
    fn one_enormous_line_cannot_take_out_the_context() {
        // The line *count* cap is no protection here: a minified bundle, a one-line
        // JSON dump or a base64 blob is a single line megabytes long, and 250 lines
        // lets all of it through.
        let bundle = format!("var x={};\n", "a".repeat(500_000));
        let out = format_read(&bundle, None, None);
        assert!(out.len() < MAX_LINE_LENGTH * 2, "the whole bundle came through: {} bytes", out.len());
        assert!(
            out.contains("line truncated"),
            "the line was cut without saying so, so the agent cannot tell it is \
             looking at half a line: {}",
            &out[..out.len().min(200)]
        );
    }

    #[test]
    fn an_ordinary_line_is_left_exactly_as_it_is() {
        let out = format_read("fn main() {}\n", None, None);
        assert!(out.contains("fn main() {}"));
        assert!(!out.contains("truncated"));
    }

    #[test]
    fn clipping_never_splits_a_character() {
        // A cut in the middle of a multi-byte character panics on slicing.
        let wide = format!("{}\n", "é".repeat(MAX_LINE_LENGTH));
        let out = format_read(&wide, None, None);
        assert!(out.contains("line truncated"), "{out:.120}");
    }
}

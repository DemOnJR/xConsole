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


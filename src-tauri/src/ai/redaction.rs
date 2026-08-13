//! High-confidence redaction for text that may become durable agent context.

const REDACTED: &str = "[REDACTED]";

const MARKERS: &[&str] = &[
    "authorization: bearer ",
    "proxy-authorization: bearer ",
    "authorization: basic ",
    "x-api-key:",
    "api-key:",
    "--password=",
    "--password ",
    "--token=",
    "--token ",
    "--api-key=",
    "--api-key ",
    "password=",
    "password:",
    "passwd=",
    "passwd:",
    "token=",
    "token:",
    "api_key=",
    "api_key:",
    "apikey=",
    "apikey:",
    "secret=",
    "secret:",
    "access_token=",
    "client_secret=",
    "connection_string=",
    "database_url=",
    "dsn=",
    "\"password\":",
    "\"passwd\":",
    "\"token\":",
    "\"api_key\":",
    "\"secret\":",
    "\"access_token\":",
];

pub fn redact_text(input: &str) -> String {
    let without_pem = redact_pem(input);
    let without_urls = redact_url_userinfo(&without_pem);
    let mut output = without_urls;
    for marker in MARKERS {
        output = redact_marker(&output, marker);
    }
    redact_mysql_password_flag(&output)
}

fn redact_pem(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(start_rel) = input[cursor..].find("-----BEGIN ") {
        let start = cursor + start_rel;
        let Some(end_rel) = input[start..].find("-----END ") else {
            break;
        };
        let end_start = start + end_rel;
        // The first "-----" in this slice is the start of "-----END ". Skip that
        // and close on the trailing dashes of "-----END <LABEL>-----".
        let after_label = end_start + "-----END ".len();
        let Some(end_line_rel) = input[after_label..].find("-----") else {
            break;
        };
        let end = after_label + end_line_rel + 5;
        output.push_str(&input[cursor..start]);
        output.push_str(REDACTED);
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn redact_url_userinfo(input: &str) -> String {
    let mut spans = Vec::new();
    let mut search = 0;
    while let Some(rel) = input[search..].find("://") {
        let scheme_end = search + rel;
        let start = scheme_end + 3;
        let end = input[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']'))
            .map(|n| start + n)
            .unwrap_or(input.len());
        if let Some(at_rel) = input[start..end].find('@') {
            let at = start + at_rel;
            if let Some(colon_rel) = input[start..at].find(':') {
                let password_start = start + colon_rel + 1;
                if password_start < at {
                    spans.push((password_start, at));
                }
            }
        }
        search = end;
    }
    apply_spans(input, &spans)
}

fn redact_marker(input: &str, marker: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(rel) = find_ascii_case_insensitive(&input[cursor..], marker) {
        let marker_start = cursor + rel;
        let value_start = marker_start + marker.len();
        if marker_start > 0
            && marker.as_bytes()[0].is_ascii_alphanumeric()
            && input.as_bytes()[marker_start - 1].is_ascii_alphanumeric()
        {
            cursor = value_start;
            continue;
        }
        let value = skip_whitespace(input, value_start);
        if input[value..].starts_with(REDACTED) {
            cursor = value + REDACTED.len();
            continue;
        }
        let end = value_end(input, value);
        if value == end {
            cursor = value_start;
            continue;
        }
        output.push_str(&input[cursor..value]);
        output.push_str(REDACTED);
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn redact_mysql_password_flag(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    if !["mysql", "mariadb", "psql", "redis-cli"]
        .iter()
        .any(|name| lower.contains(name))
    {
        return input.to_string();
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(rel) = input[cursor..].find("-p") {
        let start = cursor + rel;
        let before_ok = start == 0
            || input.as_bytes()[start - 1].is_ascii_whitespace()
            || input.as_bytes()[start - 1] == b'=';
        if !before_ok {
            cursor = start + 2;
            continue;
        }
        let value = skip_whitespace(input, start + 2);
        if input[value..].starts_with(REDACTED) {
            cursor = value + REDACTED.len();
            continue;
        }
        if value == value_end(input, value) || input[value..].starts_with(char::is_numeric) {
            cursor = start + 2;
            continue;
        }
        let end = value_end(input, value);
        output.push_str(&input[cursor..value]);
        output.push_str(REDACTED);
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn skip_whitespace(input: &str, start: usize) -> usize {
    input[start..]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| start + i)
        .unwrap_or(input.len())
}

fn value_end(input: &str, start: usize) -> usize {
    if start >= input.len() {
        return start;
    }
    let bytes = input.as_bytes();
    if bytes[start] == b'"' || bytes[start] == b'\'' {
        let quote = bytes[start];
        let mut i = start + 1;
        while i < bytes.len() {
            if bytes[i] == quote && bytes[i.saturating_sub(1)] != b'\\' {
                return i + 1;
            }
            i += 1;
        }
        return input.len();
    }
    input[start..]
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || matches!(c, ';' | '|' | '&' | ')' | ']' | ','))
        .map(|(i, _)| start + i)
        .unwrap_or(input.len())
}

fn apply_spans(input: &str, spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return input.to_string();
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for &(start, end) in spans {
        if start < cursor || end <= start || end > input.len() {
            continue;
        }
        output.push_str(&input[cursor..start]);
        output.push_str(REDACTED);
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    haystack.as_bytes().windows(needle.len()).position(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::redact_text;

    #[test]
    fn redacts_assignments_headers_urls_and_pem() {
        let input = concat!(
            "PASSWORD=hunter2 Authorization: Bearer abc123 ",
            "postgres://admin:secret@example.invalid/db ",
            "-----BEGIN PRIVATE KEY-----\\nprivate\\n-----END PRIVATE KEY-----"
        );
        let output = redact_text(input);
        assert!(!output.contains("hunter2"));
        assert!(!output.contains("abc123"));
        assert!(!output.contains("admin:secret"));
        assert!(!output.contains("PRIVATE KEY"));
        assert!(output.contains("PASSWORD=[REDACTED]"));
    }

    #[test]
    fn redacts_database_password_flags_without_touching_ports() {
        assert_eq!(
            redact_text("mysql -u root -pHUNTER2 -P3306"),
            "mysql -u root -p[REDACTED] -P3306"
        );
        assert_eq!(redact_text("docker run -p3306:3306 example"), "docker run -p3306:3306 example");
        assert_eq!(redact_text("find . -print"), "find . -print");
    }

    #[test]
    fn redaction_is_idempotent_and_preserves_safe_errors() {
        let safe = "error: command not found: systemctl";
        assert_eq!(redact_text(safe), safe);
        let redacted = redact_text("token=fixture-token");
        assert_eq!(redact_text(&redacted), redacted);
    }
}

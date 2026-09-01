//! Learning the user, not just the task.
//!
//! `reflection.rs` already learns from what went wrong — failed tools, thrashing,
//! hitting the iteration cap. What nothing learned was the *user*: their habits and
//! standing preferences only reached `TASTE.md` if the agent happened to remember to
//! call `taste_save`, which in practice it mostly did not. So the same correction got
//! made every week and the agent never appeared to know anyone.
//!
//! This reads the user's own words for durable instructions and records them.
//!
//! # Precision over recall, deliberately
//!
//! A captured preference goes into every future prompt, so a false positive is
//! expensive and roughly permanent — it quietly steers the agent wrong forever, and
//! the user has no reason to suspect a preferences file they never wrote. A missed
//! one costs a single repeated correction. Every rule here is therefore narrow:
//! explicit standing-instruction phrasing only, never an inference from what the user
//! merely happened to do.

/// Longest a captured preference may be.
///
/// A standing instruction is a sentence. A paragraph is a task description that
/// happens to contain "always", and storing it would put a whole request into the
/// permanent prompt.
const MAX_PREFERENCE_CHARS: usize = 160;

/// Shortest that still carries meaning — "never" alone is not an instruction.
const MIN_PREFERENCE_CHARS: usize = 12;

/// Openers that mark a durable instruction rather than a one-off request.
///
/// Each is anchored at the start of a sentence: "always restart nginx after this" is
/// a standing rule, whereas "check whether it always restarts" is not.
const STANDING_MARKERS: &[&str] = &[
    "always ",
    "never ",
    "from now on",
    "in future",
    "in the future",
    "going forward",
    "i prefer ",
    "i'd prefer ",
    "i would prefer ",
    "i like ",
    "i don't like ",
    "i do not like ",
    "i hate ",
    "remember that ",
    "remember to ",
    "please remember",
    "don't ever ",
    "do not ever ",
    "stop ",
    "quit ",
    "my preference is",
    "prefer ",
    "use ",
    "default to ",
];

/// Markers that only count when the sentence is clearly an instruction, because they
/// are common in ordinary requests too. Kept separate so the general set can stay
/// aggressive without dragging these in.
const WEAK_MARKERS: &[&str] = &["use ", "prefer ", "stop ", "quit "];

/// A sentence containing any of these is about the task in front of us, not a
/// standing rule — "use the staging box for this one", "always check before you
/// restart it" said mid-task.
const ONE_OFF_MARKERS: &[&str] = &[
    "for this one",
    "just this once",
    "for now",
    "this time",
    "right now",
    "today only",
];

/// Split into sentences on terminators and newlines.
fn sentences(text: &str) -> Vec<&str> {
    text.split(['.', '!', '\n', '\r', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract durable preferences from one user message.
///
/// Returns the sentences worth remembering, normalised but otherwise in the user's
/// own words — a paraphrase would be a second chance to get it wrong.
pub fn detect_preferences(user_message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for sentence in sentences(user_message) {
        let lower = sentence.to_lowercase();

        // A question is asking about a preference, not stating one: "should I always
        // use systemd here?" must not be recorded as "always use systemd".
        if sentence.contains('?') {
            continue;
        }
        if ONE_OFF_MARKERS.iter().any(|m| lower.contains(m)) {
            continue;
        }
        if sentence.chars().count() > MAX_PREFERENCE_CHARS
            || sentence.chars().count() < MIN_PREFERENCE_CHARS
        {
            continue;
        }

        let Some(marker) = STANDING_MARKERS.iter().find(|m| lower.starts_with(**m)) else {
            continue;
        };
        // The weak markers ("use x", "stop y") open plenty of ordinary one-off
        // requests, so they need a second signal that this is a standing rule.
        if WEAK_MARKERS.contains(marker) {
            let has_standing_signal = ["always", "never", "by default", "from now on", "every time"]
                .iter()
                .any(|s| lower.contains(s));
            if !has_standing_signal {
                continue;
            }
        }
        let cleaned = sentence.trim().trim_end_matches(',').to_string();
        if !out.iter().any(|e: &String| e.eq_ignore_ascii_case(&cleaned)) {
            out.push(cleaned);
        }
    }
    out
}

/// Detect preferences in the latest user turn and record them in `TASTE.md`.
///
/// Returns what was newly saved, so the turn can tell the user it learned something
/// rather than editing their profile silently.
pub fn capture_preferences(home: &crate::ai::AgentHome, user_message: &str) -> Vec<String> {
    let found = detect_preferences(user_message);
    let mut saved = Vec::new();
    for pref in found {
        // `taste::append` dedups, so re-stating a preference does not duplicate it.
        match crate::ai::taste::append(home, &pref) {
            Ok(_) => saved.push(pref),
            Err(e) => crate::diag(&format!("learn: could not save a preference: {e}")),
        }
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_explicit_standing_instructions() {
        for msg in [
            "always use k3s instead of docker compose",
            "never restart the database host without asking me",
            "from now on put deployment notes in the project brief",
            "I prefer short answers with the command first",
            "remember that the staging box has no swap",
        ] {
            assert_eq!(detect_preferences(msg).len(), 1, "missed: {msg}");
        }
    }

    #[test]
    fn ignores_questions_about_preferences() {
        // "should I always use systemd here?" is asking, not instructing — recording
        // it would invert the user's actual preference.
        for msg in [
            "should I always use systemd here?",
            "do you never restart services automatically?",
            "would you prefer json output?",
        ] {
            assert!(detect_preferences(msg).is_empty(), "false positive: {msg}");
        }
    }

    #[test]
    fn ignores_one_off_instructions() {
        // The distinction that matters most: a rule for right now is not a rule.
        for msg in [
            "always use the staging box for this one",
            "never mind the tests for now",
            "use verbose output just this once",
        ] {
            assert!(detect_preferences(msg).is_empty(), "false positive: {msg}");
        }
    }

    #[test]
    fn ignores_mid_sentence_marker_words() {
        // "always" in the middle of a sentence is describing, not instructing.
        for msg in [
            "the cron job always fails on Sundays",
            "check whether nginx always restarts cleanly",
            "it never comes back up after a reboot",
        ] {
            assert!(detect_preferences(msg).is_empty(), "false positive: {msg}");
        }
    }

    #[test]
    fn weak_markers_need_a_standing_signal() {
        // "use X" opens a great many ordinary requests.
        assert!(detect_preferences("use the backup from yesterday").is_empty());
        assert!(detect_preferences("stop the nginx service").is_empty());
        // With an explicit standing signal it is a rule.
        assert_eq!(
            detect_preferences("use pnpm by default, never npm").len(),
            1
        );
    }

    #[test]
    fn a_whole_paragraph_is_not_a_preference() {
        // Long text containing "always" is a task description; storing it would put a
        // whole request into every future prompt.
        let long = format!("always {}", "do the thing carefully and ".repeat(20));
        assert!(detect_preferences(&long).is_empty());
    }

    #[test]
    fn a_fragment_is_not_a_preference() {
        assert!(detect_preferences("never").is_empty());
        assert!(detect_preferences("always do").is_empty());
    }

    #[test]
    fn finds_several_across_a_multi_sentence_message() {
        let msg = "always use k3s here. the deploy failed again. \
                   never touch the db host without asking.";
        let found = detect_preferences(msg);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("always use k3s"));
        assert!(found[1].starts_with("never touch the db"));
    }

    #[test]
    fn keeps_the_users_own_words() {
        // A paraphrase is a second chance to get the preference wrong.
        let found = detect_preferences("I prefer short answers with the command first");
        assert_eq!(found[0], "I prefer short answers with the command first");
    }

    #[test]
    fn does_not_repeat_the_same_preference_within_one_message() {
        let found = detect_preferences("always use pnpm\nalways use pnpm");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn an_ordinary_request_yields_nothing() {
        for msg in [
            "check disk space on both servers",
            "why is nginx returning 502",
            "deploy the latest build and tail the logs",
            "",
        ] {
            assert!(detect_preferences(msg).is_empty(), "false positive: {msg}");
        }
    }
}

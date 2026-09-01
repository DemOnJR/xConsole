//! The emoji the bridge puts on a message the moment it picks it up.
//!
//! A turn can take a minute, and until it answers there is nothing on the phone to say
//! the message was even read — the user is left staring at a sent tick, unable to tell a
//! working agent from a bridge that is down. The typing indicator half-covers this, but
//! it is invisible in a notification, gone as soon as the app is backgrounded, and
//! WhatsApp's does not survive the queue behind another turn.
//!
//! A reaction does survive. It lands on *their* message, stays there, and is readable
//! from the chat list without opening anything. Which emoji it is says what the agent
//! understood the job to be, so a wrong reading — a "check the logs" that came back as a
//! restart — is visible before the work is done rather than after.
//!
//! # Why the emoji differs per platform
//!
//! Telegram only accepts reactions from a fixed list (`setMessageReaction` rejects
//! anything else outright), and 🚀 is not on it. WhatsApp and Discord take any emoji. So
//! the *meaning* is chosen once, here, and each platform renders it with the closest
//! character it will actually accept — rather than every platform being limited to
//! Telegram's list, or Telegram silently getting no reaction at all.

use super::Kind;

/// What the agent was asked to do, as far as one line of chat can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Deploy,
    Restart,
    Fix,
    Data,
    Search,
    Inspect,
    /// Nothing recognisable — still acknowledged, because "I have it" is the point.
    Other,
}

/// Words that mean the same job in the two languages this is used in. Romanian is
/// written both with and without diacritics in practice, so both spellings are listed;
/// matching is on the lowercased message.
const DEPLOY: &[&str] = &[
    "deploy", "deployeaza", "deployează", "build", "buildeste", "buildește", "release",
    "publica", "publică", "publish", "rollout", "roll out", "urca pe", "compileaza",
    "compilează", "compile",
];
const RESTART: &[&str] = &[
    "restart", "restarteaza", "restartează", "reporneste", "repornește", "reboot",
    "reload", "opreste", "oprește", "porneste", "pornește", "systemctl", "scale",
];
const FIX: &[&str] = &[
    "fix", "repara", "reparat", "eroare", "erori", "error", "broken", "stricat", "bug",
    "crash", "nu merge", "nu functioneaza", "nu funcționează", "failed", "esuat",
    "eșuat", "debug", "cade", "picat", "down",
];
const DATA: &[&str] = &[
    "backup", "baza de date", "database", "db ", "sql", "mysql", "mariadb", "postgres",
    "dump", "restaureaza", "restaurează", "restore", "migrare", "migration", "tabel",
    "table",
];
const SEARCH: &[&str] = &[
    "cauta", "caută", "search", "find", "gaseste", "găsește", "google", "research",
    "cerceteaza", "cercetează", "documenteaza", "documentează", "afla",
];
const INSPECT: &[&str] = &[
    "verifica", "verifică", "check", "status", "log", "loguri", "uita-te", "uită-te",
    "vezi", "arata", "arată", "show", "list", "listeaza", "listează", "monitor",
    "cat ", "citeste", "citește", "read", "cum sta", "cum stă", "ce face",
];

/// Read one chat message as a kind of job.
///
/// Order is by specificity, not by list length: "verifică de ce nu merge nginx" is a
/// fix, not an inspection, and "build and restart" is a deploy. A miss costs a slightly
/// wrong emoji, never a wrong action — nothing downstream reads this.
pub fn classify(text: &str) -> Task {
    let lower = format!(" {} ", text.to_lowercase());
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(FIX) {
        Task::Fix
    } else if has(DEPLOY) {
        Task::Deploy
    } else if has(DATA) {
        Task::Data
    } else if has(RESTART) {
        Task::Restart
    } else if has(SEARCH) {
        Task::Search
    } else if has(INSPECT) {
        Task::Inspect
    } else {
        Task::Other
    }
}

/// The character to send, for a platform that only takes what it knows.
///
/// Telegram's allowed set is the constraint; every emoji in that column is one
/// `setMessageReaction` accepts from a non-premium bot.
pub fn emoji(kind: Kind, task: Task) -> &'static str {
    match (kind, task) {
        (Kind::Telegram, Task::Deploy) => "🔥",
        (Kind::Telegram, Task::Restart) => "⚡",
        (Kind::Telegram, Task::Fix) => "👨‍💻",
        (Kind::Telegram, Task::Data) => "🐳",
        (Kind::Telegram, Task::Search) => "🤔",
        (Kind::Telegram, Task::Inspect) => "👀",
        (Kind::Telegram, Task::Other) => "👌",
        (_, Task::Deploy) => "🚀",
        (_, Task::Restart) => "🔄",
        (_, Task::Fix) => "🛠️",
        (_, Task::Data) => "💾",
        (_, Task::Search) => "🔎",
        (_, Task::Inspect) => "👀",
        (_, Task::Other) => "👌",
    }
}

/// What to put on a message that has just been accepted, for the platform it arrived on.
pub fn for_message(kind: Kind, text: &str) -> &'static str {
    emoji(kind, classify(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_job_is_read_out_of_the_message() {
        assert_eq!(classify("deployeaza csb pe k3s"), Task::Deploy);
        assert_eq!(classify("restarteaza nginx"), Task::Restart);
        assert_eq!(classify("fa un backup la baza de date"), Task::Data);
        assert_eq!(classify("cauta pe net cum se face"), Task::Search);
        assert_eq!(classify("verifica logurile"), Task::Inspect);
        assert_eq!(classify("salut"), Task::Other);
    }

    #[test]
    fn a_broken_thing_is_a_fix_even_when_it_says_check() {
        // "verifică de ce nu merge" is the most common way work arrives, and reading it
        // as an inspection would put 👀 on every outage.
        assert_eq!(classify("verifica de ce nu merge site-ul"), Task::Fix);
        assert_eq!(classify("build and restart, ceva e stricat"), Task::Fix);
    }

    #[test]
    fn diacritics_are_optional_because_nobody_types_them_on_a_phone() {
        assert_eq!(classify("repornește serverul"), Task::Restart);
        assert_eq!(classify("reporneste serverul"), Task::Restart);
    }

    #[test]
    fn telegram_only_gets_emoji_it_will_accept() {
        // setMessageReaction rejects the whole request for an emoji outside its list, so
        // an expressive one there means no reaction at all — the exact silence this
        // feature exists to end.
        const TELEGRAM_ALLOWED: &[&str] = &[
            "👍", "👎", "❤", "🔥", "🥰", "👏", "😁", "🤔", "🤯", "😱", "🤬", "😢", "🎉",
            "🤩", "🙏", "👌", "🕊", "🤡", "🥱", "🥴", "😍", "🐳", "🌚", "🌭", "💯", "🤣",
            "⚡", "🍌", "🏆", "💔", "🤨", "😐", "🍓", "🍾", "💋", "🖕", "😈", "😴", "😭",
            "🤓", "👻", "👨‍💻", "👀", "🎃", "🙈", "😇", "😨", "🤝", "✍", "🤗", "🫡",
            "🎅", "🎄", "☃", "💅", "🤪", "🗿", "🆒", "💘", "🙉", "🦄", "😘", "💊", "🙊",
            "😎", "👾", "🤷", "😡",
        ];
        for task in [
            Task::Deploy,
            Task::Restart,
            Task::Fix,
            Task::Data,
            Task::Search,
            Task::Inspect,
            Task::Other,
        ] {
            let e = emoji(Kind::Telegram, task);
            assert!(TELEGRAM_ALLOWED.contains(&e), "telegram would refuse {e} for {task:?}");
        }
    }

    #[test]
    fn every_platform_reacts_to_every_kind_of_job() {
        // A missing arm would mean one platform silently stops acknowledging, which is
        // indistinguishable from the bridge being down.
        for kind in Kind::ALL {
            for task in [Task::Deploy, Task::Fix, Task::Other] {
                assert!(!emoji(kind, task).is_empty());
            }
        }
    }
}

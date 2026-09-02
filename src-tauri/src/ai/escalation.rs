//! The agent the team answers to looks at anything that cannot be undone, before the
//! user is asked about it.
//!
//! A destructive command reaching the user as a bare yes/no puts the whole judgement on
//! a person reading one line on a phone, usually while doing something else. The reply
//! everyone gives in that position is "yes" — which makes the prompt a formality rather
//! than a check, and a formality is not a safeguard.
//!
//! So the lead reads it first, with what it would destroy in front of it, and answers one
//! of two things: this is what the work needs, or it is not. A refusal ends there and the
//! user is told what was stopped. Anything else still goes to the user, now carrying the
//! lead's line — the confirmation is theirs to give, and it is worth more when somebody
//! has already looked.
//!
//! # What this is not
//!
//! Not a substitute for the user's yes, and not a way to run something they never saw. It
//! can only *narrow* what is put to them: every path that is not an explicit refusal ends
//! at the same question in the chat. If there is nobody to ask, if the model is
//! unreachable, or if the answer is unreadable, the command goes to the user exactly as
//! it would have without any of this.

use crate::ai::persona;
use crate::ai::provider::{ChatMessage, ChatRequest};
use crate::ai::registry;
use crate::storage::models::Persona;
use crate::storage::Db;

/// Names the persona that reviews destructive work, when it should not simply be the top
/// of the chain of command.
pub const SETTING_REVIEWER: &str = "remote.review_persona_id";

/// How long the reviewer has before the question goes to the user anyway.
///
/// A review that has not answered is not a refusal. Waiting longer than this would turn
/// a slow model into a silent block on work the user is waiting for.
const REVIEW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Kept short on purpose: the verdict is a line, and a reviewer that writes an essay is
/// answering the wrong question.
const MAX_TOKENS: u32 = 400;

/// What the reviewing agent said about one command.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// The agent that looked at it, by name, so the user knows who is vouching.
    pub reviewer: String,
    /// Whether it should be put to the user at all.
    pub ok: bool,
    /// Their one line of reasoning, as they wrote it.
    pub note: String,
}

impl Verdict {
    /// The line the user reads in the chat.
    pub fn line(&self) -> String {
        let verb = if self.ok { "checked it and says it is fine" } else { "refused it" };
        match self.note.trim() {
            "" => format!("{} {verb}.", self.reviewer),
            note => format!("{} {verb}: {note}", self.reviewer),
        }
    }
}

/// Who should look at a destructive command proposed by `running`.
///
/// In order: the agent the user named for this; the top of the proposing agent's chain of
/// command; or, when nobody is named and nobody is running, the single agent that answers
/// to the user directly. `None` means there is nobody to ask — including the case that
/// matters most, where the proposer *is* the top of the chain: an agent reviewing itself
/// is not a review, and the answer is already in what it proposed.
pub fn reviewer(db: &Db, running: Option<&str>) -> Option<Persona> {
    let all: Vec<Persona> = db
        .list_personas()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.enabled)
        .collect();
    let not_self = |p: Persona| (Some(p.id.as_str()) != running).then_some(p);

    if let Ok(Some(named)) = db.get_setting(SETTING_REVIEWER) {
        if !named.trim().is_empty() {
            return persona::resolve(db, named.trim()).filter(|p| p.enabled).and_then(not_self);
        }
    }

    if let Some(me) = running.and_then(|id| all.iter().find(|p| p.id == id)) {
        if let Some(top) = persona::chain_to_top(&all, me).last() {
            return not_self((*top).clone());
        }
    }

    let tops: Vec<&Persona> = all.iter().filter(|p| persona::is_top_level(p)).collect();
    match tops.as_slice() {
        [only] => not_self((*only).clone()),
        // Several agents answer to the user, and picking one of them at random would put
        // an arbitrary name on a verdict. The user names one, or nobody reviews.
        _ => None,
    }
}

/// Ask the reviewer whether this command should be put to the user.
///
/// `shown` is the redacted command together with its blast radius — the same text the
/// user would see. `asked` is what the user wanted, which is what makes the difference
/// between a delete that is the job and a delete that is a mistake.
pub async fn review(
    db: &Db,
    running: Option<&str>,
    shown: &str,
    asked: &str,
) -> Option<Verdict> {
    let reviewer = reviewer(db, running)?;
    let provider_id = registry::active_provider_id(db, reviewer.provider_id.as_deref()).ok()?;
    let resolved = registry::build_with_model(db, &provider_id, reviewer.model.as_deref()).ok()?;

    let mut system = format!("You are {}", reviewer.name);
    if !reviewer.role.trim().is_empty() {
        system.push_str(&format!(", {}", reviewer.role.trim()));
    }
    system.push_str(
        ". One of the agents you are responsible for wants to run a command that cannot be \
         undone, and it does not run until you have looked at it.\n\n\
         Judge one thing: is this what the work in front of you actually needs? A delete \
         that is the job is fine. What is not fine is a path that looks wrong for what was \
         asked, a scope wider than the request, a target that reads like production when \
         the request did not mention it, or a measurement that says there is far more \
         there than the task implies.\n\n\
         You are not the last check — the user is asked next, and they decide. Refuse only \
         what you would not want put in front of them.\n\n\
         Answer in exactly two lines:\n\
         OK or NO\n\
         one short sentence, in the language the request was written in, saying why.",
    );
    if !reviewer.instructions.trim().is_empty() {
        system.push_str("\n\nYour standing instructions:\n");
        system.push_str(&crate::ai::text::keep_newest(reviewer.instructions.trim(), 4000));
    }

    let mut req = ChatRequest::new(&resolved.model);
    req.system = system;
    req.messages = vec![ChatMessage::user(format!(
        "What the user asked for:\n{}\n\nWhat the agent wants to run:\n{}",
        crate::ai::text::keep_newest(asked.trim(), 2000),
        crate::ai::text::keep_newest(shown.trim(), 4000),
    ))];
    req.temperature = 0.2;
    req.max_tokens = MAX_TOKENS;

    let answer = match tokio::time::timeout(REVIEW_TIMEOUT, resolved.provider.chat(&req, None)).await
    {
        Ok(Ok(resp)) => resp.content,
        // A reviewer that could not be reached has not refused anything. The command goes
        // to the user, which is where it was going before any of this existed.
        Ok(Err(e)) => {
            crate::diag(&format!("escalation: {} could not review it: {e}", reviewer.name));
            return None;
        }
        Err(_) => {
            crate::diag(&format!("escalation: {} did not answer in time", reviewer.name));
            return None;
        }
    };

    parse(&reviewer.name, &answer)
}

/// Read a verdict out of the reply.
///
/// Only an explicit refusal counts as one. An answer that is neither word is not a veto
/// and not an endorsement: it is a reviewer that did not answer the question, and the
/// command goes to the user unannotated rather than carrying a line nobody can read.
fn parse(reviewer: &str, answer: &str) -> Option<Verdict> {
    let mut lines = answer.trim().lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next()?;
    let head: String = first
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect::<String>()
        .to_uppercase();
    let ok = match head.as_str() {
        "OK" | "YES" | "DA" => true,
        "NO" | "NU" => false,
        _ => return None,
    };
    // The reason is the rest of the first line when it carries one, else the next line.
    let tail = first[head.len().min(first.len())..]
        .trim_start_matches([':', '.', ',', '-', ' '])
        .trim();
    let note = if tail.is_empty() { lines.next().unwrap_or_default() } else { tail };
    Some(Verdict {
        reviewer: reviewer.to_string(),
        ok,
        note: note.chars().take(400).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_is_read_from_the_first_word() {
        let v = parse("Adrian", "OK - e chiar directorul de build, se poate").unwrap();
        assert!(v.ok);
        assert_eq!(v.reviewer, "Adrian");
        assert!(v.note.starts_with("e chiar"), "{}", v.note);

        let v = parse("Adrian", "NO\n/srv is not what the task asked for").unwrap();
        assert!(!v.ok);
        assert_eq!(v.note, "/srv is not what the task asked for");

        assert!(parse("Adrian", "DA\nasta e").unwrap().ok);
        assert!(!parse("Adrian", "nu, calea e gresita").unwrap().ok);
    }

    #[test]
    fn an_unreadable_answer_is_not_a_verdict() {
        // Neither a veto nor an endorsement. Treating a rambling answer as approval would
        // put the lead's name on something it never said, and treating it as a refusal
        // would block work on a model that simply answered badly.
        assert!(parse("Adrian", "I think we should probably look at this more").is_none());
        assert!(parse("Adrian", "").is_none());
    }

    fn team() -> (Db, Persona, Persona) {
        use crate::storage::models::PersonaInput;
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        let mk = |name: &str, reports_to: Option<String>| {
            db.upsert_persona(&PersonaInput {
                id: None,
                name: name.into(),
                role: String::new(),
                instructions: String::new(),
                targets: vec![],
                safety_mode: None,
                provider_id: None,
                model: None,
                enabled: true,
                reports_to,
                workspace_id: None,
                allowed_paths: Vec::new(),
                allowed_tools: Vec::new(),
            })
            .unwrap()
        };
        let lead = mk("Adrian", None);
        let engineer = mk("Gabriel", Some(lead.id.clone()));
        (db, lead, engineer)
    }

    #[test]
    fn work_is_reviewed_by_the_agent_at_the_top_of_the_chain() {
        let (db, lead, engineer) = team();
        assert_eq!(reviewer(&db, Some(&engineer.id)).map(|p| p.name), Some("Adrian".into()));
        // Nobody running: one agent answers to the user, so it is unambiguous.
        assert_eq!(reviewer(&db, None).map(|p| p.name), Some("Adrian".into()));
    }

    #[test]
    fn the_top_of_the_chain_does_not_review_itself() {
        // The lead proposing it has already made the judgement this would be asking for,
        // and a rubber stamp with its own name on it reads to the user like a second
        // opinion. Straight to the user instead.
        let (db, lead, _) = team();
        assert!(reviewer(&db, Some(&lead.id)).is_none());
    }

    #[test]
    fn an_ambiguous_org_chart_names_nobody_until_the_user_does() {
        use crate::storage::models::PersonaInput;
        let (db, _lead, engineer) = team();
        let second = db
            .upsert_persona(&PersonaInput {
                id: None,
                name: "Ioana".into(),
                role: String::new(),
                instructions: String::new(),
                targets: vec![],
                safety_mode: None,
                provider_id: None,
                model: None,
                enabled: true,
                reports_to: None,
                workspace_id: None,
                allowed_paths: Vec::new(),
                allowed_tools: Vec::new(),
            })
            .unwrap();
        // Two agents answer to the user; picking one would put an arbitrary name on a
        // verdict. With a proposer there is still a chain to walk, so that keeps working.
        assert_eq!(reviewer(&db, Some(&engineer.id)).map(|p| p.name), Some("Adrian".into()));
        assert!(reviewer(&db, None).is_none());

        // Until the user says who reviews.
        db.set_setting(SETTING_REVIEWER, &second.id).unwrap();
        assert_eq!(reviewer(&db, Some(&engineer.id)).map(|p| p.name), Some("Ioana".into()));
        // And even a named reviewer never reviews its own proposal.
        assert!(reviewer(&db, Some(&second.id)).is_none());
    }

    #[test]
    fn the_line_names_who_looked_at_it() {
        let v = Verdict { reviewer: "Adrian".into(), ok: true, note: "e ok".into() };
        assert!(v.line().starts_with("Adrian"), "{}", v.line());
        assert!(v.line().contains("e ok"));
        let no = Verdict { reviewer: "Adrian".into(), ok: false, note: String::new() };
        assert!(no.line().contains("refused"), "{}", no.line());
    }
}

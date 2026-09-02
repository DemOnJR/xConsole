//! Coming back when the work is done.
//!
//! A task delegated from a chat used to end in silence. The remote turn answered "I have
//! sent that to Ada, I will let you know" and then every record that a human was waiting
//! — the turn context, the transport, the chat — was dropped the instant the turn
//! returned. The goal loop reached `done`, wrote a row, emitted an event the canvas node
//! consumed, and stopped. There was no second outbound path at all.
//!
//! This is that path. Every way a run can end funnels through [`on_goal_terminal`], and
//! it has exactly two answers:
//!
//! - The agent has a manager, so the result goes up the chain — the same
//!   message-and-wake the agents already use to talk to each other. The ask travels with
//!   it, so when the chain reaches somebody who answers to the user, the user is still
//!   who it is answered to.
//! - Otherwise the result is queued for the chat that asked, in the outbox, which is a
//!   table rather than a channel because the restart that loses an in-memory queue is
//!   exactly the one that happens while a long task is running.
//!
//! # Why the template is written first
//!
//! The body is composed deterministically and written to the row *before* the model is
//! asked to improve it. A provider outage, a rate limit or a timeout then costs a
//! well-phrased sentence — not the notification. Making the model the author of the only
//! copy would reintroduce the failure this module exists to remove, in a form that is
//! harder to see.

use chrono::Utc;

use crate::ai::goal::GoalContext;
use crate::storage::database::{OutboxMessage, RemoteRequest};
use crate::storage::models::{GoalSession, Persona};
use crate::storage::Db;

/// How long several agents finishing one ask are gathered into one message.
///
/// Short enough that the phone buzzes while the user still remembers asking, long enough
/// that a fan-out of five delegations does not arrive as five notifications.
const DEBOUNCE_SECS: i64 = 20;

/// How long an ask may look unattended before it is called abandoned.
///
/// Covers the window between recording the ask and the first delegated task existing,
/// plus a restart. Below this a perfectly healthy turn would be announced as lost.
const ABANDON_GRACE_MINS: i64 = 2;

/// Longest a queued body may be. Phones, and a body that grows every time a sibling
/// finishes.
const MAX_BODY: usize = 3000;

/// Where a finished run's result should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// Up the chain of command, to somebody who can act on it.
    ToManager { id: String, name: String },
    /// Out to a human, in a chat. `request` is present when a specific ask is being
    /// answered — which is also what closes that ask once the message lands.
    ToChat {
        request: Option<RemoteRequest>,
        transport: String,
        chat_id: String,
    },
    /// Nobody is waiting. A desktop run that finished cleanly is on the board already.
    Nothing,
}

/// Decide who hears about a finished run.
///
/// Pure apart from reads, so the routing can be tested without a running app — which is
/// the half most likely to be wrong.
pub fn plan(db: &Db, goal: &GoalSession) -> Report {
    // Once is the contract. Everything downstream is also idempotent, but a task whose
    // result was already passed on must not pass it on again on the next hook.
    if goal.reported_at.is_some() {
        return Report::Nothing;
    }

    let personas = db.list_personas().unwrap_or_default();
    let me = goal
        .persona_id
        .as_deref()
        .and_then(|id| personas.iter().find(|p| p.id == id));
    if let Some(m) = me.and_then(|p| crate::ai::persona::manager_of(&personas, p)) {
        return Report::ToManager {
            id: m.id.clone(),
            name: m.name.clone(),
        };
    }

    // The ask this work belongs to, if a human started it from a phone.
    if let Some(req) = goal
        .request_id
        .as_deref()
        .and_then(|id| db.get_remote_request(id).ok().flatten())
    {
        return Report::ToChat {
            transport: req.transport.clone(),
            chat_id: req.chat_id.clone(),
            request: Some(req),
        };
    }

    // Nobody asked for this over chat — a standing duty, or a desktop /goal. A clean
    // finish is already on the board and needs no notification; a run that gave up is
    // the case the old `goal://notify` was reaching for and never delivered, so it goes
    // wherever the user last spoke, if anywhere.
    if goal.status == "blocked" {
        if let Some(route) = crate::ai::remote::last_route(db) {
            return Report::ToChat {
                request: None,
                transport: route.kind.as_str().to_string(),
                chat_id: route.chat_id,
            };
        }
    }
    Report::Nothing
}

/// What a finished run has to say, in words that do not need a model to produce.
///
/// This is the copy that actually gets delivered. Anything the model adds later is an
/// improvement on it, never a replacement for having one.
pub fn summary(goal: &GoalSession, who: Option<&str>) -> String {
    let name = who.unwrap_or("The agent");
    let head = match goal.status.as_str() {
        "done" => format!("{name} finished: {}", goal.title.trim()),
        "blocked" => format!("{name} is stuck on: {}", goal.title.trim()),
        "stopped" => format!("{name} stopped: {}", goal.title.trim()),
        other => format!("{name} ended ({other}): {}", goal.title.trim()),
    };
    let body = goal
        .outcome
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(match goal.status.as_str() {
            "done" => "It reported no detail beyond finishing.",
            _ => "It recorded no reason. Open the task to see what it ran.",
        });
    clip(&format!("{head}\n\n{body}"), MAX_BODY)
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Queue one message, or fold it into the one already waiting.
///
/// Returns the row id when this call is the one that created it. `None` means somebody
/// beat us to the same logical event — two agents finishing one ask, or the same
/// terminal state hooked twice — which is the debounce doing its job, not a failure.
pub fn enqueue(
    db: &Db,
    dedupe_key: &str,
    transport: &str,
    chat_id: &str,
    request_id: Option<&str>,
    goal_id: Option<&str>,
    body: &str,
    delay_secs: i64,
) -> Option<String> {
    let row = OutboxMessage {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: request_id.map(str::to_string),
        goal_id: goal_id.map(str::to_string),
        transport: transport.to_string(),
        chat_id: chat_id.to_string(),
        body: clip(body, MAX_BODY),
        state: "pending".to_string(),
        attempts: 0,
        next_attempt_at: (delay_secs > 0).then(|| {
            (Utc::now() + chrono::Duration::seconds(delay_secs))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        }),
        last_error: None,
        dedupe_key: dedupe_key.to_string(),
        created_at: None,
        sent_at: None,
    };
    match db.enqueue_outbox(&row) {
        Ok(true) => Some(row.id),
        Ok(false) => {
            // Already queued. Add to it while it is still waiting, so the second agent's
            // result rides along instead of being dropped — one message, both answers.
            let _ = db.append_outbox_body(dedupe_key, &clip(body, MAX_BODY));
            None
        }
        Err(e) => {
            crate::diag(&format!("outbox: could not queue {dedupe_key}: {e}"));
            None
        }
    }
}

/// The dedupe key for "this ask has been answered". One per ask, so a fan-out of
/// delegations comes back as one message.
pub fn key_for(goal: &GoalSession, request: Option<&RemoteRequest>) -> String {
    match request {
        Some(r) => format!("req:{}", r.id),
        None => format!("goal:{}", goal.id),
    }
}

/// Every way a run can end arrives here, exactly once.
pub async fn on_goal_terminal(ctx: &GoalContext, goal: &mut GoalSession) {
    let plan = plan(&ctx.db, goal);
    if matches!(plan, Report::Nothing) {
        // Still stamped: "considered and there was nobody to tell" is a decision, and
        // leaving it null would have every later hook reconsider it.
        goal.reported_at = Some(Utc::now().to_rfc3339());
        return;
    }
    goal.reported_at = Some(Utc::now().to_rfc3339());

    let personas = ctx.db.list_personas().unwrap_or_default();
    let me = goal
        .persona_id
        .as_deref()
        .and_then(|id| personas.iter().find(|p| p.id == id));
    let body = summary(goal, me.map(|p| p.name.as_str()));

    match plan {
        Report::Nothing => {}
        Report::ToManager { id, name } => {
            hand_up(ctx, goal, me, &id, &body);
            crate::diag(&format!("goal {}: reported to {name}", goal.id));
        }
        Report::ToChat {
            request,
            transport,
            chat_id,
        } => {
            let key = key_for(goal, request.as_ref());
            let queued = enqueue(
                &ctx.db,
                &key,
                &transport,
                &chat_id,
                request.as_ref().map(|r| r.id.as_str()),
                Some(goal.id.as_str()),
                &body,
                DEBOUNCE_SECS,
            );
            // The deterministic body is already on the row and already deliverable. Only
            // now, and only when a person is actually waiting on this one, is the model
            // asked to say it better.
            if let (Some(row_id), Some(req)) = (queued, request.as_ref()) {
                if let Some(better) = polish(&ctx.db, me, req, goal, &body).await {
                    let _ = ctx.db.update_outbox_body(&row_id, &better);
                }
            }
        }
    }
}

/// Pass a result to the manager, the way the agents already talk to each other.
///
/// The ask rides along: the manager's own run is stamped with it, so when the chain
/// reaches somebody who answers to the user, the outbox still knows which chat that is.
fn hand_up(
    ctx: &GoalContext,
    goal: &GoalSession,
    me: Option<&Persona>,
    manager_id: &str,
    body: &str,
) {
    use tauri::Emitter;
    let msg = crate::storage::models::AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        from_id: me.map(|p| p.id.clone()),
        to_id: Some(manager_id.to_string()),
        kind: "report".to_string(),
        body: body.to_string(),
        goal_id: Some(goal.id.clone()),
        workspace_id: goal.workspace_id.clone(),
        read_at: None,
        created_at: None,
        channel_id: None,
        parent_id: None,
        mentions: Vec::new(),
        resolved_at: None,
    };
    if let Err(e) = ctx.db.insert_agent_message(&msg) {
        crate::diag(&format!("goal {}: could not file the report: {e}", goal.id));
        return;
    }
    let _ = ctx.app.emit("agent://message", &msg);
    // Held across nothing that awaits: the wake is synchronous, and this is a
    // process-wide slot.
    let _scope = crate::ai::remote::RequestScope::enter(goal.request_id.clone());
    crate::ai::persona_tools::wake_persona(&ctx.app, &ctx.db, manager_id);
}

/// Deliver something an agent addressed to the user, from wherever it was written.
///
/// The caller this exists for is `agent_report`'s no-manager branch: an agent that
/// answers to nobody files its report with `to_id = NULL` and then emits `goal://notify`
/// — an event that appears in no frontend file at all, so the report is written down and
/// never read. One call here puts the same words on the path that reaches a phone.
///
/// Keyed on the words, so a cycle that reports the same thing twice says it once.
pub fn report_to_user(db: &Db, goal_id: Option<&str>, from: &str, body: &str) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let goal = goal_id.and_then(|id| db.get_goal(id).ok().flatten());
    let request = goal
        .as_ref()
        .and_then(|g| g.request_id.as_deref())
        .and_then(|id| db.get_remote_request(id).ok().flatten());
    let (transport, chat_id) = match request.as_ref() {
        Some(r) => (r.transport.clone(), r.chat_id.clone()),
        // Nobody asked over chat, so this goes wherever the user last spoke — or
        // nowhere, which is the honest answer when they have never used a bridge.
        None => {
            let route = crate::ai::remote::last_route(db)?;
            (route.kind.as_str().to_string(), route.chat_id)
        }
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut h);
    enqueue(
        db,
        &format!("say:{}:{:x}", goal_id.unwrap_or("-"), h.finish()),
        &transport,
        &chat_id,
        request.as_ref().map(|r| r.id.as_str()),
        goal_id,
        &format!("{from}: {body}"),
        0,
    )
}

/// Adopt a task into the ask that is being answered right now.
///
/// The seam is deliberate. `ToolContext` cannot carry this — a task outlives the turn
/// that started it, and the only record that survives a restart is the goal row itself.
/// So the ownership is stamped at the one point every delegated task passes through:
/// the moment its loop is started. Never re-parented, so a task begun for one ask is
/// not stolen by the next.
pub fn adopt_request(db: &Db, goal_id: &str) {
    let Some(request_id) = crate::ai::remote::current_request_id() else {
        return;
    };
    match db.set_goal_request(goal_id, &request_id) {
        Ok(true) => crate::diag(&format!("goal {goal_id}: answering request {request_id}")),
        Ok(false) => {}
        Err(e) => crate::diag(&format!("goal {goal_id}: could not record the asker: {e}")),
    }
}

/// Nothing may end in silence.
///
/// Runs on the existing ticker. Two failures it exists to catch: an ask whose work
/// vanished — a crash, a goal deleted, a loop that never started — and an ask whose work
/// is genuinely taking hours, where saying nothing is indistinguishable from the first
/// case to the person waiting.
pub fn sweep_unanswered(db: &Db) {
    let Ok(open) = db.list_open_remote_requests() else {
        return;
    };
    let now = Utc::now();
    for req in open {
        let age = req
            .created_at
            .as_deref()
            .and_then(crate::ai::persona_tools::parse_goal_ts)
            .map(|t| now.signed_duration_since(t))
            .unwrap_or_else(chrono::Duration::zero);
        if age.num_minutes() < ABANDON_GRACE_MINS {
            continue;
        }
        let goals = db.goals_for_request(&req.id).unwrap_or_default();
        let live = goals.iter().any(|g| {
            matches!(g.status.as_str(), "intake" | "active" | "waiting" | "paused")
        });
        let queued = db.outbox_live_for_request(&req.id).unwrap_or(false);

        if !live && !queued {
            let what = if goals.is_empty() {
                "Nothing was started for it".to_string()
            } else {
                format!(
                    "The {} task(s) started for it are no longer running and reported nothing",
                    goals.len()
                )
            };
            enqueue(
                db,
                &format!("req:{}", req.id),
                &req.transport,
                &req.chat_id,
                Some(&req.id),
                goals.first().map(|g| g.id.as_str()),
                &format!(
                    "I did not get back to you about this and I should have.\n\n\
                     You asked: {}\n\n{what}. Ask again and it will be picked up from \
                     scratch.",
                    clip(req.ask.trim(), 300)
                ),
                0,
            );
            continue;
        }

        // Still working. An hourly line, keyed on the hour so the ticker cannot repeat
        // it: long work and abandoned work look identical from a phone otherwise.
        let hours = age.num_hours();
        if live && hours >= 1 {
            let titles: Vec<&str> = goals
                .iter()
                .filter(|g| matches!(g.status.as_str(), "intake" | "active" | "waiting" | "paused"))
                .map(|g| g.title.trim())
                .take(4)
                .collect();
            enqueue(
                db,
                &format!("hb:{}:{hours}", req.id),
                &req.transport,
                &req.chat_id,
                // A heartbeat is not an answer, so it must not settle the ask on its way
                // out — the point of it is that the ask is still open.
                None,
                None,
                &format!(
                    "Still on it after {hours}h — not forgotten.\n\n\
                     You asked: {}\n\nRunning now: {}",
                    clip(req.ask.trim(), 300),
                    if titles.is_empty() { "(a task with no title)".to_string() } else { titles.join("; ") }
                ),
                0,
            );
        }
    }
}

/// How long the model gets to improve a message that is already deliverable.
///
/// Shorter than the debounce on purpose: the worker will send the draft the moment the
/// debounce is up, and a rewrite that arrives after that is simply dropped. Losing the
/// nicer wording is the correct outcome; waiting for it would not be.
const POLISH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Say it the way the agent would have said it, if the provider is up.
///
/// Tool-less and bounded, and any failure at all returns `None`, leaving the
/// deterministic body exactly where it already is: on the row, queued, about to be sent.
async fn polish(
    db: &Db,
    me: Option<&Persona>,
    request: &RemoteRequest,
    goal: &GoalSession,
    draft: &str,
) -> Option<String> {
    use crate::ai::provider::{ChatMessage, ChatRequest};
    use crate::ai::registry;

    let provider_id = registry::active_provider_id(db, me.and_then(|p| p.provider_id.as_deref())).ok()?;
    let resolved = registry::build_with_model(db, &provider_id, me.and_then(|p| p.model.as_deref())).ok()?;

    let mut req = ChatRequest::new(&resolved.model);
    req.system = format!(
        "You are {}. You are writing one short message to the person who asked you for \
         this work, and they will read it on a phone.\n\n\
         Rewrite the draft below into at most four short lines of plain text. Keep every \
         concrete fact — what was done, what was not, any number or name in it. Add \
         nothing that is not in the draft: no invented detail, no reassurance, no \
         apology. Answer in the language the request was written in. Output the message \
         and nothing else.",
        me.map(|p| p.name.as_str()).unwrap_or("the agent"),
    );
    req.messages = vec![ChatMessage::user(format!(
        "What they asked for:\n{}\n\nDraft:\n{}",
        clip(request.ask.trim(), 600),
        clip(draft, MAX_BODY),
    ))];
    req.temperature = 0.2;
    req.max_tokens = 400;

    let text = match tokio::time::timeout(POLISH_TIMEOUT, resolved.provider.chat(&req, None)).await {
        Ok(Ok(r)) => r.content,
        Ok(Err(e)) => {
            crate::diag(&format!("goal {}: report not polished: {e}", goal.id));
            return None;
        }
        Err(_) => {
            crate::diag(&format!("goal {}: report polish timed out", goal.id));
            return None;
        }
    };
    let text = text.trim();
    // An empty answer, or one longer than what it was meant to shorten, is not an
    // improvement — and the draft is already good enough to send.
    (!text.is_empty() && text.chars().count() <= MAX_BODY).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::PersonaInput;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn goal(id: &str, status: &str) -> GoalSession {
        GoalSession {
            id: id.to_string(),
            title: format!("task {id}"),
            raw_request: "look at the disk on web-1".into(),
            spec_json: "{}".into(),
            status: status.to_string(),
            kanban_json: "[]".into(),
            memory_json: "{}".into(),
            next_check_at: None,
            cycles: 1,
            created_at: None,
            updated_at: None,
            finished_at: None,
            persona_id: None,
            workspace_id: None,
            outcome: Some(format!("what {id} found")),
            request_id: None,
            reported_at: None,
            pr_number: None,
            approval_state: None,
        }
    }

    fn persona(d: &Db, id: &str, name: &str, reports_to: Option<&str>) -> Persona {
        d.upsert_persona(&PersonaInput {
            id: Some(id.into()),
            name: name.into(),
            role: String::new(),
            instructions: String::new(),
            targets: vec![],
            safety_mode: None,
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: reports_to.map(str::to_string),
            workspace_id: None,
            allowed_paths: vec![],
            allowed_tools: vec![],
        })
        .unwrap()
    }

    fn asked(d: &Db, id: &str) -> RemoteRequest {
        asked_at(d, id, 0)
    }

    /// An ask recorded `mins_ago` minutes ago, so the sweep's age rules are testable
    /// without waiting for them.
    fn asked_at(d: &Db, id: &str, mins_ago: i64) -> RemoteRequest {
        let when = (Utc::now() - chrono::Duration::minutes(mins_ago))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let r = RemoteRequest {
            id: id.to_string(),
            transport: "whatsapp".into(),
            chat_id: "40712345678@s.whatsapp.net".into(),
            author_id: "40712345678".into(),
            message_id: Some("m1".into()),
            persona_id: None,
            ask: "check the disk on web-1".into(),
            status: "open".into(),
            created_at: Some(when),
            closed_at: None,
        };
        d.insert_remote_request(&r).unwrap();
        r
    }

    /// The whole point of the request row: the answer goes to the person who asked.
    #[test]
    fn a_finished_task_is_routed_to_the_chat_that_asked_for_it() {
        let d = db();
        let req = asked(&d, "req-1");
        let mut g = goal("g1", "done");
        g.request_id = Some(req.id.clone());
        d.insert_goal(&g).unwrap();

        match plan(&d, &g) {
            Report::ToChat { transport, chat_id, request } => {
                assert_eq!(transport, "whatsapp");
                assert_eq!(chat_id, "40712345678@s.whatsapp.net");
                assert_eq!(request.map(|r| r.id).as_deref(), Some("req-1"));
            }
            other => panic!("expected the asking chat, got {other:?}"),
        }
    }

    #[test]
    fn an_agent_with_a_manager_reports_up_instead_of_messaging_the_user() {
        // A hierarchy exists so the user is not the first person to hear about every
        // detail. An engineer's result goes to their lead, who decides what is worth
        // passing on -- and the ask travels with it, so when the chain reaches somebody
        // who answers to the user, the user is still who gets answered.
        let d = db();
        let boss = persona(&d, "p-lead", "Ada", None);
        let hand = persona(&d, "p-eng", "Grace", Some(&boss.id));
        let req = asked(&d, "req-2");

        let mut g = goal("g1", "done");
        g.persona_id = Some(hand.id.clone());
        g.request_id = Some(req.id.clone());
        d.insert_goal(&g).unwrap();
        assert_eq!(
            plan(&d, &g),
            Report::ToManager { id: boss.id.clone(), name: "Ada".into() },
            "a report, not a message to the phone"
        );

        // The lead answers to nobody, so their own ending is where the user hears.
        let mut theirs = goal("g2", "done");
        theirs.persona_id = Some(boss.id);
        theirs.request_id = Some(req.id);
        assert!(matches!(plan(&d, &theirs), Report::ToChat { .. }));
    }

    #[test]
    fn two_agents_finishing_one_ask_send_one_message_carrying_both() {
        // Five delegations from one question must not buzz a phone five times -- and
        // must not silently drop four results to achieve that.
        let d = db();
        let req = asked(&d, "req-3");
        let mut first = goal("g1", "done");
        first.request_id = Some(req.id.clone());
        first.outcome = Some("web-1 is at 91%".into());
        let mut second = goal("g2", "done");
        second.request_id = Some(req.id.clone());
        second.outcome = Some("web-2 is fine".into());

        let key = key_for(&first, Some(&req));
        assert_eq!(key, key_for(&second, Some(&req)), "one ask, one key");

        let a = enqueue(&d, &key, "whatsapp", &req.chat_id, Some(&req.id), Some("g1"),
                        &summary(&first, Some("Grace")), 20);
        let b = enqueue(&d, &key, "whatsapp", &req.chat_id, Some(&req.id), Some("g2"),
                        &summary(&second, Some("Ada")), 20);
        assert!(a.is_some(), "the first ending queues the message");
        assert!(b.is_none(), "the second joins it rather than queueing a second one");

        let queued = d.list_outbox(100).unwrap();
        assert_eq!(queued.len(), 1, "one message: {queued:?}");
        assert!(queued[0].body.contains("web-1 is at 91%"));
        assert!(queued[0].body.contains("web-2 is fine"), "nothing is dropped to get there");
    }

    #[test]
    fn the_same_ending_hooked_twice_queues_one_message() {
        // The hook can run twice: a loop resumed after a restart re-reads a goal that is
        // already terminal. The unique key is what makes that harmless.
        let d = db();
        let req = asked(&d, "req-4");
        let mut g = goal("g1", "done");
        g.request_id = Some(req.id.clone());
        let key = key_for(&g, Some(&req));
        let body = summary(&g, Some("Ada"));

        assert!(enqueue(&d, &key, "whatsapp", &req.chat_id, Some(&req.id), Some("g1"), &body, 20).is_some());
        assert!(enqueue(&d, &key, "whatsapp", &req.chat_id, Some(&req.id), Some("g1"), &body, 20).is_none());
        assert_eq!(d.list_outbox(100).unwrap().len(), 1);

        // And a run that has already reported does not report again at all.
        g.reported_at = Some(Utc::now().to_rfc3339());
        assert_eq!(plan(&d, &g), Report::Nothing);
    }

    #[test]
    fn a_run_that_gave_up_says_so_rather_than_ending_quietly() {
        // Blocked is the ending most worth hearing about and the one that used to emit a
        // `goal://notify` event no frontend file listened for.
        let d = db();
        let req = asked(&d, "req-5");
        let mut g = goal("g1", "blocked");
        g.request_id = Some(req.id.clone());
        g.outcome = Some("Stopped after 5 cycles that changed nothing".into());
        d.insert_goal(&g).unwrap();

        let body = summary(&g, Some("Ada"));
        assert!(body.contains("stuck"), "{body}");
        assert!(body.contains("5 cycles"), "the reason travels with it: {body}");
        assert!(matches!(plan(&d, &g), Report::ToChat { .. }));

        // Nobody asked over chat and there is no manager: a stuck run still goes
        // wherever the user last spoke, which is what the dead notification meant to do.
        let mut orphan = goal("g2", "blocked");
        orphan.persona_id = None;
        assert_eq!(plan(&d, &orphan), Report::Nothing, "nowhere to write yet");
        crate::ai::remote::remember_route(
            &d,
            &crate::ai::remote::Route {
                kind: crate::ai::remote::Kind::Telegram,
                chat_id: "tg-1".into(),
            },
        );
        assert!(matches!(plan(&d, &orphan), Report::ToChat { request: None, .. }));

        // A clean desktop finish is on the board already and needs no phone message.
        assert_eq!(plan(&d, &goal("g3", "done")), Report::Nothing);
    }

    #[test]
    fn an_ask_whose_work_vanished_is_admitted_to() {
        let d = db();
        // Past the grace window, which exists so a turn that is still running is never
        // announced as lost.
        let req = asked_at(&d, "req-6", 30);
        sweep_unanswered(&d);
        let queued = d.list_outbox(100).unwrap();
        assert_eq!(queued.len(), 1, "silence is not an option: {queued:?}");
        assert_eq!(queued[0].chat_id, req.chat_id);
        assert!(queued[0].body.contains("did not get back to you"), "{}", queued[0].body);

        // Said once, not once every thirty seconds.
        sweep_unanswered(&d);
        assert_eq!(d.list_outbox(100).unwrap().len(), 1);
    }

    #[test]
    fn work_still_running_is_left_alone_until_it_has_been_an_hour() {
        let d = db();
        let req = asked_at(&d, "req-7", 30);
        let mut g = goal("g1", "active");
        g.request_id = Some(req.id.clone());
        d.insert_goal(&g).unwrap();

        sweep_unanswered(&d);
        assert!(d.list_outbox(100).unwrap().is_empty(), "a task in flight is not news");

        // An hour of nothing is indistinguishable from an abandoned ask, from a phone.
        let old = asked_at(&d, "req-7b", 70);
        let mut g2 = goal("g2", "active");
        g2.request_id = Some(old.id.clone());
        d.insert_goal(&g2).unwrap();
        sweep_unanswered(&d);
        let queued = d.list_outbox(100).unwrap();
        assert_eq!(queued.len(), 1);
        assert!(queued[0].body.contains("Still on it"), "{}", queued[0].body);
        sweep_unanswered(&d);
        assert_eq!(d.list_outbox(100).unwrap().len(), 1, "one an hour, not one a tick");
    }

    #[test]
    fn a_report_addressed_to_the_user_reaches_their_chat() {
        // An agent at the top of the chain reports to the user. That used to mean a row
        // with a null recipient and an event nothing listened for.
        let d = db();
        let req = asked(&d, "req-9");
        let mut g = goal("g1", "done");
        g.request_id = Some(req.id.clone());
        d.insert_goal(&g).unwrap();

        assert!(report_to_user(&d, Some("g1"), "Ada", "web-1 is back up").is_some());
        let queued = d.list_outbox(100).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].chat_id, req.chat_id);
        assert!(queued[0].body.contains("Ada: web-1 is back up"));

        // Said once, however many times the agent repeats itself.
        assert!(report_to_user(&d, Some("g1"), "Ada", "web-1 is back up").is_none());
        assert_eq!(d.list_outbox(100).unwrap().len(), 1);

        // And with no chat anywhere in the picture there is nothing to claim.
        let empty = db();
        assert!(report_to_user(&empty, None, "Ada", "anything").is_none());
    }

    #[tokio::test]
    async fn the_message_is_written_before_any_model_is_asked_to_improve_it() {
        // The failure this whole module exists to remove is silence. Composing with a
        // model first would put the notification behind a provider that can be down,
        // rate-limited or slow -- and reintroduce it in a form that only shows up on a
        // bad day.
        let d = db();
        let req = asked(&d, "req-8");
        let mut g = goal("g1", "done");
        g.request_id = Some(req.id.clone());
        let draft = summary(&g, Some("Ada"));
        let id = enqueue(&d, &key_for(&g, Some(&req)), "whatsapp", &req.chat_id,
                         Some(&req.id), Some("g1"), &draft, 20)
            .expect("queued");
        assert_eq!(d.get_outbox(&id).unwrap().unwrap().body, draft, "deliverable already");

        // No provider is configured, which is the same thing that happens when one is
        // configured and unreachable.
        assert!(polish(&d, None, &req, &g, &draft).await.is_none());
        assert_eq!(
            d.get_outbox(&id).unwrap().unwrap().body,
            draft,
            "a provider that cannot answer must not cost the message"
        );
    }
}

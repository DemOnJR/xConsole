//! Rooms, threads and per-agent log channels.
//!
//! A project is a server, and inside it are channels: `#general` for the team and one
//! live log channel per agent, readable by everyone, where anybody can open a thread on
//! a specific line to add a correction.
//!
//! Kept out of `persona.rs` on purpose. That file is CRUD for named agents plus the
//! user's inbox; routing, waking and read cursors are a different subject with a
//! different failure mode, and the two grew tangled once already.
//!
//! ## The bug this file exists to fix
//!
//! `post_agent_message` only wakes an agent when `to_id` is set, and `to_id = NULL`
//! already means "addressed to the user". So a message typed into `#company` or into a
//! project room was stored, echoed back into the UI, and delivered to nobody: the rooms
//! were write-only and looked as though they worked. A room post has no single
//! recipient by definition, so who to wake has to be derived from the room itself --
//! that is `wake_targets`.

use std::collections::HashSet;

use tauri::{AppHandle, Emitter, State};

use crate::storage::models::{AgentLogEntry, AgentMessage, ChannelUnread};
use crate::storage::Db;

/// A channel id, parsed.
///
/// Channel ids are a plain-string grammar rather than rows in a `channel` table: the
/// set of rooms is a pure function of the projects and the agents that already exist,
/// so a table would only be a second copy of that to be kept in sync -- and every
/// creation and deletion of a project would need a matching room migration.
///
/// A message with `channel_id IS NULL` predates rooms and is routed by the client's
/// original derivation instead, so upgrading does not hide existing history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRef {
    /// Everyone, across every project.
    Company,
    /// A project's `#general`.
    ProjectGeneral { workspace_id: String },
    /// One agent's live log, inside a project.
    AgentLog {
        workspace_id: String,
        persona_id: String,
    },
    /// A lead and their reports.
    Team { lead_id: String },
    /// One-to-one with the user.
    Dm { persona_id: String },
}

/// Longest channel id worth entertaining: two uuids plus the grammar around them.
const MAX_CHANNEL_ID: usize = 200;

/// Parse a channel id, or refuse it.
///
/// Refusing is the point. Without this an id is whatever string the caller sent, rows
/// accumulate under `ws::general` and `dm:` and typos of both, and the room they belong
/// to is unrecoverable afterwards because nothing ever wrote down what was intended.
pub fn parse_channel(id: &str) -> Option<ChannelRef> {
    let id = id.trim();
    if id.is_empty() || id.len() > MAX_CHANNEL_ID {
        return None;
    }
    let parts: Vec<&str> = id.split(':').collect();
    let filled = |s: &str| !s.trim().is_empty();
    match parts.as_slice() {
        ["company"] => Some(ChannelRef::Company),
        ["ws", ws, "general"] if filled(ws) => Some(ChannelRef::ProjectGeneral {
            workspace_id: (*ws).to_string(),
        }),
        ["ws", ws, "log", p] if filled(ws) && filled(p) => Some(ChannelRef::AgentLog {
            workspace_id: (*ws).to_string(),
            persona_id: (*p).to_string(),
        }),
        ["team", lead] if filled(lead) => Some(ChannelRef::Team {
            lead_id: (*lead).to_string(),
        }),
        ["dm", p] if filled(p) => Some(ChannelRef::Dm {
            persona_id: (*p).to_string(),
        }),
        _ => None,
    }
}

/// The project a room belongs to, so a post is stamped with what it is about.
fn workspace_of(ch: &ChannelRef) -> Option<String> {
    match ch {
        ChannelRef::ProjectGeneral { workspace_id } => Some(workspace_id.clone()),
        ChannelRef::AgentLog { workspace_id, .. } => Some(workspace_id.clone()),
        _ => None,
    }
}

/// Who has to start running because of this post.
///
/// A message nobody is running to read is a message that never arrived, and a room post
/// has no `to_id` to wake. So the room says who:
///
/// - anyone named in the message, wherever it was said;
/// - the owner of a `ws:*:log:<id>` channel, because a correction on their log line is
///   for them;
/// - every agent assigned to the project behind `ws:*:general`;
/// - the recipient of a DM, and the lead of a team room.
///
/// `#company` deliberately wakes only the people named in it. Waking the entire staff
/// on every broadcast would turn one sentence into a fleet-wide billable cycle, which
/// is a worse failure than a note read on the next cycle.
///
/// The author is never woken by their own words.
pub fn wake_targets(
    db: &Db,
    ch: &ChannelRef,
    mentions: &[String],
    author: Option<&str>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |id: String, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if Some(id.as_str()) == author {
            return;
        }
        if seen.insert(id.clone()) {
            out.push(id);
        }
    };

    for m in mentions {
        if let Some(p) = crate::ai::persona::resolve(db, m) {
            push(p.id, &mut out, &mut seen);
        }
    }

    match ch {
        ChannelRef::Company => {}
        ChannelRef::Dm { persona_id } | ChannelRef::AgentLog { persona_id, .. } => {
            if let Some(p) = crate::ai::persona::resolve(db, persona_id) {
                push(p.id, &mut out, &mut seen);
            }
        }
        ChannelRef::Team { lead_id } => {
            if let Some(p) = crate::ai::persona::resolve(db, lead_id) {
                push(p.id, &mut out, &mut seen);
            }
        }
        ChannelRef::ProjectGeneral { workspace_id } => {
            // Assigned membership only. The UI infers a wider membership from goals and
            // server overlap to decide who to *show*; inferring who to *bill a cycle to*
            // from the same guess would wake half the company off a shared VPS list.
            for p in db.list_personas().unwrap_or_default() {
                if p.workspace_id.as_deref() == Some(workspace_id.as_str()) {
                    push(p.id, &mut out, &mut seen);
                }
            }
        }
    }
    out
}

/// One room's messages, oldest last. Replies live in their thread, not in the room.
#[tauri::command]
pub async fn list_channel_messages(
    db: State<'_, Db>,
    channel_id: String,
    limit: Option<i64>,
) -> Result<Vec<AgentMessage>, String> {
    if parse_channel(&channel_id).is_none() {
        return Err(format!("not a channel id: {channel_id}"));
    }
    db.list_channel_messages(&channel_id, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Everything hanging off one message or one log line, oldest first.
#[tauri::command]
pub async fn list_channel_thread(
    db: State<'_, Db>,
    parent_id: String,
) -> Result<Vec<AgentMessage>, String> {
    let parent = parent_id.trim();
    if parent.is_empty() {
        return Err("a thread needs a parent".into());
    }
    db.list_thread(parent).map_err(|e| e.to_string())
}

/// What one agent has been doing, oldest last.
#[tauri::command]
pub async fn list_agent_log(
    db: State<'_, Db>,
    persona_id: String,
    limit: Option<i64>,
) -> Result<Vec<AgentLogEntry>, String> {
    let id = persona_id.trim();
    if id.is_empty() {
        return Err("which agent?".into());
    }
    db.list_agent_log(id, limit.unwrap_or(300))
        .map_err(|e| e.to_string())
}

/// Per-room unread counts for one reader. `reader_id` empty (or absent) is the user.
#[tauri::command]
pub async fn channel_unread(
    db: State<'_, Db>,
    reader_id: Option<String>,
) -> Result<Vec<ChannelUnread>, String> {
    let reader = reader_id.unwrap_or_default();
    db.channel_unread_counts(reader.trim())
        .map_err(|e| e.to_string())
}

/// Move a reader's cursor in a room to now.
#[tauri::command]
pub async fn mark_channel_read(
    db: State<'_, Db>,
    channel_id: String,
    reader_id: Option<String>,
) -> Result<(), String> {
    if parse_channel(&channel_id).is_none() {
        return Err(format!("not a channel id: {channel_id}"));
    }
    let reader = reader_id.unwrap_or_default();
    db.mark_channel_read(reader.trim(), channel_id.trim())
        .map_err(|e| e.to_string())
}

/// Post into a room, or into a thread hanging off a message or a log line.
///
/// This is the write path the Teams view uses instead of guessing at
/// `(to_id, workspace_id, kind)`, and unlike `post_agent_message` it wakes somebody:
/// see `wake_targets`.
#[tauri::command]
pub async fn post_channel_message(
    app: AppHandle,
    db: State<'_, Db>,
    channel_id: String,
    body: String,
    parent_id: Option<String>,
    mentions: Option<Vec<String>>,
    kind: Option<String>,
) -> Result<AgentMessage, String> {
    let Some(ch) = parse_channel(&channel_id) else {
        return Err(format!("not a channel id: {channel_id}"));
    };
    let body = body.trim();
    if body.is_empty() {
        return Err("message is empty".into());
    }
    let kind = kind
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "note".into());
    if !matches!(kind.as_str(), "note" | "request" | "report") {
        return Err("kind must be note, request, or report".into());
    }

    // A parent is not checked for existence on purpose. It may name an `agent_message`,
    // an `agent_log` row, or -- until activity is persisted -- a log line that so far
    // exists only in the live status feed. Refusing the third case would remove the
    // reply affordance from exactly the lines the user most wants to correct, so an
    // unresolvable parent is allowed and the client degrades it to a top-level post in
    // the same room rather than dropping it. Only the shape is enforced.
    let parent = parent_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if parent.as_deref().is_some_and(|p| p.len() > MAX_CHANNEL_ID) {
        return Err("that is not a message id".into());
    }

    // Only real people get mentioned. Storing an unresolvable id would raise a mention
    // badge on a reader who does not exist and wake nobody.
    let mentions: Vec<String> = mentions
        .unwrap_or_default()
        .iter()
        .filter_map(|m| crate::ai::persona::resolve(&db, m).map(|p| p.id))
        .collect();
    let mentions = {
        let mut seen = HashSet::new();
        mentions
            .into_iter()
            .filter(|m| seen.insert(m.clone()))
            .collect::<Vec<_>>()
    };

    let msg = AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        // The user is the only author who posts through this command; agents write
        // through their own tools.
        from_id: None,
        to_id: None,
        kind,
        body: body.to_string(),
        goal_id: None,
        workspace_id: workspace_of(&ch),
        read_at: None,
        created_at: None,
        channel_id: Some(channel_id.trim().to_string()),
        parent_id: parent,
        mentions: mentions.clone(),
        resolved_at: None,
    };
    db.insert_agent_message(&msg).map_err(|e| e.to_string())?;
    // Read back so the event carries the timestamp the row actually holds. Cursors
    // compare against `created_at` as a string, so an invented one drifts from it.
    let stored = db
        .get_agent_message(&msg.id)
        .map_err(|e| e.to_string())?
        .unwrap_or(msg);
    let _ = app.emit("agent://message", &stored);

    for id in wake_targets(&db, &ch, &mentions, None) {
        crate::ai::persona_tools::wake_persona(&app, &db, &id);
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::PersonaInput;

    fn db() -> Db {
        Db::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn hire(d: &Db, id: &str, name: &str, workspace: Option<&str>) {
        d.upsert_persona(&PersonaInput {
            id: Some(id.into()),
            name: name.into(),
            role: "engineer".into(),
            instructions: String::new(),
            targets: Vec::new(),
            safety_mode: None,
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: None,
            workspace_id: workspace.map(|s| s.to_string()),
            allowed_paths: Vec::new(),
            allowed_tools: Vec::new(),
        })
        .unwrap();
    }

    #[test]
    fn the_grammar_round_trips_every_kind_of_room() {
        assert_eq!(parse_channel("company"), Some(ChannelRef::Company));
        assert_eq!(
            parse_channel("ws:k8s:general"),
            Some(ChannelRef::ProjectGeneral {
                workspace_id: "k8s".into()
            })
        );
        assert_eq!(
            parse_channel("ws:k8s:log:ada"),
            Some(ChannelRef::AgentLog {
                workspace_id: "k8s".into(),
                persona_id: "ada".into()
            })
        );
        assert_eq!(
            parse_channel("team:ada"),
            Some(ChannelRef::Team {
                lead_id: "ada".into()
            })
        );
        assert_eq!(
            parse_channel("dm:ada"),
            Some(ChannelRef::Dm {
                persona_id: "ada".into()
            })
        );
    }

    /// Every one of these was a plausible typo or a truncated id. Accepting any of them
    /// files a message in a room nobody can ever open again.
    #[test]
    fn a_malformed_channel_id_is_refused_rather_than_invented() {
        for bad in [
            "",
            "   ",
            "ws",
            "ws:",
            "ws:k8s",
            "ws::general",
            "ws:k8s:general:extra",
            "ws:k8s:log",
            "ws:k8s:log:",
            "team",
            "team:",
            "dm:",
            "Company",
            "general",
            "ws:k8s:logs:ada",
        ] {
            assert_eq!(parse_channel(bad), None, "{bad:?} should be refused");
        }
        assert_eq!(parse_channel(&"x".repeat(MAX_CHANNEL_ID + 1)), None);
    }

    /// The live bug: a post into a room reached nobody, because waking was tied to
    /// `to_id` and a room post has none. The project's own agents have to be woken by
    /// the room itself.
    #[test]
    fn a_project_room_post_wakes_that_project_and_nobody_else() {
        let d = db();
        hire(&d, "ada", "Ada", Some("k8s"));
        hire(&d, "bruno", "Bruno", Some("k8s"));
        hire(&d, "carol", "Carol", Some("shop"));
        hire(&d, "dana", "Dana", None);

        let ch = parse_channel("ws:k8s:general").unwrap();
        let mut woken = wake_targets(&d, &ch, &[], None);
        woken.sort();
        assert_eq!(woken, ["ada", "bruno"]);
    }

    /// A correction on somebody's log line is addressed to them whatever room it was
    /// typed in, and a name in the body reaches its owner from anywhere.
    #[test]
    fn a_log_channel_wakes_its_owner_and_mentions_reach_across_rooms() {
        let d = db();
        hire(&d, "ada", "Ada", Some("k8s"));
        hire(&d, "carol", "Carol", Some("shop"));

        let log = parse_channel("ws:k8s:log:ada").unwrap();
        assert_eq!(wake_targets(&d, &log, &[], None), ["ada"]);

        // Mentions are resolved by name as well as by id, so "@Carol" works.
        let company = parse_channel("company").unwrap();
        assert_eq!(
            wake_targets(&d, &company, &["Carol".into()], None),
            ["carol"]
        );
        // ...and an unknown name wakes nobody rather than erroring the post away.
        assert!(wake_targets(&d, &company, &["ghost".into()], None).is_empty());
    }

    /// Waking everyone on a company-wide note would turn one sentence into a cycle per
    /// member of staff. Only the people actually named run.
    #[test]
    fn a_company_note_wakes_only_the_people_it_names() {
        let d = db();
        hire(&d, "ada", "Ada", Some("k8s"));
        hire(&d, "bruno", "Bruno", None);
        let ch = parse_channel("company").unwrap();
        assert!(wake_targets(&d, &ch, &[], None).is_empty());
        assert_eq!(wake_targets(&d, &ch, &["ada".into()], None), ["ada"]);
    }

    /// Mentioning somebody twice, or mentioning the owner of the log you are replying
    /// on, must not start two cycles for one message.
    #[test]
    fn nobody_is_woken_twice_and_the_author_never_wakes_themselves() {
        let d = db();
        hire(&d, "ada", "Ada", Some("k8s"));
        hire(&d, "bruno", "Bruno", Some("k8s"));
        let ch = parse_channel("ws:k8s:log:ada").unwrap();
        assert_eq!(
            wake_targets(&d, &ch, &["ada".into(), "Ada".into()], None),
            ["ada"]
        );
        // Bruno writing in Ada's log wakes Ada only.
        let mut woken = wake_targets(&d, &ch, &["bruno".into()], Some("bruno"));
        woken.sort();
        assert_eq!(woken, ["ada"]);
    }
}

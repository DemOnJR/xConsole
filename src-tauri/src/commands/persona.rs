//! CRUD for named agents (personas), plus the list the UI shows.

use tauri::{AppHandle, Emitter, State};

use crate::storage::models::{AgentMessage, Persona, PersonaInput};
use crate::storage::Db;

#[tauri::command]
pub async fn list_personas(db: State<'_, Db>) -> Result<Vec<Persona>, String> {
    db.list_personas().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_persona(db: State<'_, Db>, input: PersonaInput) -> Result<Persona, String> {
    save_persona_checked(&db, input)
}

/// Validate and store a persona.
///
/// Split out from the command so the agent's `agent_hire` tool goes through exactly the
/// same refusals. A second, laxer path is how a duplicate name or a reporting loop gets
/// in — and a loop means an escalation that never reaches the user.
pub fn save_persona_checked(db: &Db, input: PersonaInput) -> Result<Persona, String> {
    if input.name.trim().is_empty() {
        return Err("a persona needs a name".into());
    }
    // Names are how the user and the agent address a persona ("ask Ada to…"), so two
    // personas sharing one name makes delegation ambiguous — and `resolve` would pick
    // whichever the database happened to return first.
    if let Ok(Some(existing)) = db.get_persona_by_name(input.name.trim()) {
        if Some(&existing.id) != input.id.as_ref() {
            return Err(format!("an agent named {} already exists", existing.name));
        }
    }
    // A reporting loop would leave an escalation with no top to reach: everyone in the
    // cycle reports to someone else in it, and nothing ever gets to the user.
    if let Some(manager) = input.reports_to.as_deref().filter(|s| !s.trim().is_empty()) {
        let all = db.list_personas().map_err(|e| e.to_string())?;
        let id = input.id.clone().unwrap_or_default();
        if crate::ai::persona::would_create_cycle(&all, &id, manager) {
            return Err(
                "that reporting line would form a loop — an escalation would never reach you"
                    .into(),
            );
        }
    }
    db.upsert_persona(&input).map_err(|e| e.to_string())
}

/// The org chart as indented text, for the settings panel.
#[tauri::command]
pub async fn persona_org_chart(db: State<'_, Db>) -> Result<String, String> {
    let all = db.list_personas().map_err(|e| e.to_string())?;
    Ok(crate::ai::persona::format_org_chart(&all))
}

/// What the agents have said to each other — the whole exchange, or one task's.
#[tauri::command]
pub async fn list_agent_messages(
    db: State<'_, Db>,
    goal_id: Option<String>,
    // `workspace_id`: limit to one project. Omit to read across all of them.
    workspace_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<crate::storage::models::AgentMessage>, String> {
    db.list_agent_messages(goal_id.as_deref(), workspace_id.as_deref(), limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Messages waiting for the user (sent by a top-level agent that answers to them).
#[tauri::command]
pub async fn unread_user_messages(
    db: State<'_, Db>,
) -> Result<Vec<crate::storage::models::AgentMessage>, String> {
    db.unread_agent_messages(None).map_err(|e| e.to_string())
}

/// One agent's record over a window, for the panel that shows what it has been doing.
#[derive(serde::Serialize)]
pub struct AgentActivity {
    pub persona_id: String,
    pub name: String,
    /// The project it belongs to, if any.
    pub project: Option<String>,
    pub days: i64,
    pub tasks: Vec<crate::storage::models::GoalSession>,
    pub changes: Vec<crate::ai::edits::EditRecord>,
    pub messages: Vec<crate::storage::models::AgentMessage>,
}

/// What one agent has done lately.
///
/// The same three sources the agent's own `agent_activity` tool reads, so the panel and
/// the answer the agent gives cannot disagree about what happened.
#[tauri::command]
pub async fn agent_activity(
    db: State<'_, Db>,
    persona_id: String,
    days: Option<i64>,
) -> Result<AgentActivity, String> {
    let p = db
        .get_persona(&persona_id)
        .map_err(|e| e.to_string())?
        .ok_or("no such agent")?;
    let days = days.unwrap_or(7).clamp(1, 90);
    let since = chrono::Utc::now() - chrono::Duration::days(days);

    Ok(AgentActivity {
        project: p
            .workspace_id
            .as_deref()
            .and_then(|id| db.get_workspace(id).ok().flatten())
            .map(|w| w.name),
        tasks: db
            .agent_tasks_since(&p.id, &since.to_rfc3339())
            .map_err(|e| e.to_string())?,
        changes: db
            .agent_file_changes_since(&p.id, since.timestamp_millis(), 200)
            .map_err(|e| e.to_string())?,
        messages: db
            .agent_messages_since(&p.id, &since.to_rfc3339(), 100)
            .map_err(|e| e.to_string())?,
        persona_id: p.id,
        name: p.name,
        days,
    })
}

/// Create a standard team for a project.
///
/// The same planning and creation the agent's `team_create` tool uses, so a team built
/// from the button and one built by asking come out identical. No approval prompt here:
/// the click is the approval.
#[tauri::command]
pub async fn create_team(
    db: State<'_, Db>,
    workspace_id: String,
    roles: Option<Vec<String>>,
    about: Option<String>,
) -> Result<Vec<Persona>, String> {
    let ws = db
        .get_workspace(&workspace_id)
        .map_err(|e| e.to_string())?
        .ok_or("no such project")?;
    let roles = roles.filter(|r| !r.is_empty()).unwrap_or_else(|| {
        crate::ai::persona_tools::DEFAULT_ROLES.iter().map(|r| r.to_string()).collect()
    });
    let planned = crate::ai::persona_tools::plan_team(&db, &ws.name, &roles, about.as_deref());
    let (_made, failed) = crate::ai::persona_tools::create_team(&db, &ws.id, &planned);
    if !failed.is_empty() {
        // Partial success is still worth reporting as a failure: a team missing its
        // reviewer is not the team that was asked for.
        return Err(failed.join("; "));
    }
    db.list_personas().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_agent_messages_read(db: State<'_, Db>, ids: Vec<String>) -> Result<(), String> {
    db.mark_agent_messages_read(&ids).map_err(|e| e.to_string())
}

/// Post a message into a team thread as the user.
///
/// Agents already write via `agent_send` / `agent_report`. The teams view needs the
/// same table from the other direction: you talking to a person or to the whole
/// team (to_id empty), so the chat is not a one-way log.
#[tauri::command]
pub async fn post_agent_message(
    app: AppHandle,
    db: State<'_, Db>,
    body: String,
    to_id: Option<String>,
    workspace_id: Option<String>,
    kind: Option<String>,
) -> Result<AgentMessage, String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("message is empty".into());
    }
    let to = to_id.filter(|s| !s.trim().is_empty());
    let ws = workspace_id.filter(|s| !s.trim().is_empty());
    let kind = kind
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "note".into());
    if !matches!(kind.as_str(), "note" | "request" | "report") {
        return Err("kind must be note, request, or report".into());
    }
    let msg = AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        from_id: None,
        to_id: to,
        kind,
        body: body.to_string(),
        goal_id: None,
        workspace_id: ws,
        read_at: None,
        created_at: Some(chrono::Utc::now().to_rfc3339()),
    };
    db.insert_agent_message(&msg).map_err(|e| e.to_string())?;
    let _ = app.emit("agent://message", &msg);
    // Same as agent_send: a request sitting unread in an idle inbox is not a
    // request. Wake the named recipient so they actually run.
    if let Some(to) = msg.to_id.as_deref() {
        crate::ai::persona_tools::wake_persona(&app, &db, to);
    }
    Ok(msg)
}

#[tauri::command]
pub async fn delete_persona(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_persona(&id).map_err(|e| e.to_string())
}

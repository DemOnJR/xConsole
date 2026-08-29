//! CRUD for named agents (personas), plus the list the UI shows.

use tauri::State;

use crate::storage::models::{Persona, PersonaInput};
use crate::storage::Db;

#[tauri::command]
pub async fn list_personas(db: State<'_, Db>) -> Result<Vec<Persona>, String> {
    db.list_personas().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_persona(db: State<'_, Db>, input: PersonaInput) -> Result<Persona, String> {
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
    limit: Option<i64>,
) -> Result<Vec<crate::storage::models::AgentMessage>, String> {
    db.list_agent_messages(goal_id.as_deref(), limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// Messages waiting for the user (sent by a top-level agent that answers to them).
#[tauri::command]
pub async fn unread_user_messages(
    db: State<'_, Db>,
) -> Result<Vec<crate::storage::models::AgentMessage>, String> {
    db.unread_agent_messages(None).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_agent_messages_read(db: State<'_, Db>, ids: Vec<String>) -> Result<(), String> {
    db.mark_agent_messages_read(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_persona(db: State<'_, Db>, id: String) -> Result<(), String> {
    db.delete_persona(&id).map_err(|e| e.to_string())
}

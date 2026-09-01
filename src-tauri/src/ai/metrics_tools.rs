//! What a project earns, and whether it is going up.
//!
//! The teams exist to make the projects pay. Without figures that is unmeasurable: an
//! agent can report three fixes shipped and a clean deploy and still have no idea
//! whether anything got better, so "improve sales" degrades into "do more work". A
//! number per day per project turns it into a question with an answer.
//!
//! Deliberately not a fixed set of metrics. One project sells subscriptions, another
//! takes orders, another counts signups before anything is sold at all — the agent
//! records whatever that project actually has, and comparisons work the same either way.

use serde_json::{json, Value};

use crate::ai::provider::ToolDef;
use crate::ai::tools::ToolContext;

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "metric_record".into(),
            description: "Record one figure for one day on a project — revenue, orders, signups, \
refunds, whatever that project measures. Record what you actually read from a dashboard, a \
database or an invoice; never estimate. Recording the same day twice corrects it rather than \
adding to it, so re-reading a source is safe."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "What is being measured, e.g. \"revenue\", \"orders\", \"signups\". Keep the same name over time or the history will not line up."},
                    "value": {"type": "number", "description": "The figure for that one day."},
                    "day": {"type": "string", "description": "The day it is about, YYYY-MM-DD. Defaults to today. A figure read late still belongs to its own day."},
                    "unit": {"type": "string", "description": "e.g. \"EUR\", \"USD\", \"count\"."},
                    "note": {"type": "string", "description": "Where the number came from, so it can be checked later."},
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."}
                },
                "required": ["name", "value"]
            }),
        },
        ToolDef {
            name: "metric_source_set".into(),
            description: "Teach xConsole how to fetch one of a project's numbers, so it stops \
having to be typed in. Give a command whose output starts with the number — a SQL query \
(`mysql -N -e \"SELECT COALESCE(SUM(total),0) FROM orders WHERE DATE(created_at)=CURDATE()\"`), a \
log count, an API call piped through jq. It must be read-only; a metric that changes something \
is not a measurement. The user approves it once, then it runs unattended, so show them the \
exact command first. Pass enabled=false to stop one."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "The metric this fetches, e.g. \"revenue\"."},
                    "vps": {"type": "string", "description": "Server id to run it on. Use list_vps_targets if unsure."},
                    "command": {"type": "string", "description": "Must print one number on stdout, for the day it is run."},
                    "unit": {"type": "string", "description": "e.g. \"EUR\", \"count\"."},
                    "enabled": {"type": "boolean", "description": "False stops it without forgetting it."},
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."}
                },
                "required": ["name", "vps", "command"]
            }),
        },
        ToolDef {
            name: "metric_collect".into(),
            description: "Run every configured source for a project and record what they return. \
This is what makes the numbers keep themselves up to date — call it at the start of a review, or \
on a schedule. Reports each source that failed rather than quietly recording nothing."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."},
                    "day": {"type": "string", "description": "The day the figures are about, YYYY-MM-DD. Defaults to today."}
                }
            }),
        },
        ToolDef {
            name: "metric_trend".into(),
            description: "How a project's numbers are moving: this period against the one before \
it, per metric. This is what turns \"sales are down\" into a fact, and it is the first thing to \
look at before deciding what a team should work on. With no metric named, reports every metric \
the project has."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "One metric. Omit for all of them."},
                    "days": {"type": "integer", "description": "Length of each period in days (default 7: this week against last)."},
                    "project": {"type": "string", "description": "Project name. Defaults to the one currently open."}
                }
            }),
        },
    ]
}

pub fn is_metric_tool(name: &str) -> bool {
    matches!(
        name,
        "metric_record" | "metric_trend" | "metric_source_set" | "metric_collect"
    )
}

/// Reading a trend changes nothing; recording a figure is a durable claim about the
/// business, so plan mode withholds it.
pub fn tool_is_mutating(name: &str) -> bool {
    // Collecting writes figures, and defining a source is a standing grant to run a
    // command unattended. Only reading a trend changes nothing.
    name != "metric_trend"
}

pub async fn dispatch(ctx: &ToolContext, name: &str, args: &Value) -> String {
    match name {
        "metric_record" => record(ctx, args),
        "metric_trend" => trend(ctx, args),
        "metric_source_set" => source_set(ctx, args).await,
        "metric_collect" => collect(ctx, args).await,
        _ => format!("error: unknown metric tool {name}"),
    }
}

/// Which project a call is about: the one named, or the one open.
fn project_of(ctx: &ToolContext, args: &Value) -> Result<(String, String), String> {
    let all = ctx.db.list_workspaces().unwrap_or_default();
    let named = args.get("project").and_then(|v| v.as_str()).map(str::trim);
    let ws = match named.filter(|n| !n.is_empty()) {
        Some(n) => all
            .iter()
            .find(|w| w.name.eq_ignore_ascii_case(n) || w.id == n)
            .ok_or_else(|| {
                format!(
                    "no project called {n:?}. Known: {}",
                    all.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            })?,
        None => {
            let here = ctx
                .workspace_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("no project is open, so name one with `project`")?;
            all.iter().find(|w| w.id == here).ok_or("the open project no longer exists")?
        }
    };
    Ok((ws.id.clone(), ws.name.clone()))
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn record(ctx: &ToolContext, args: &Value) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_lowercase();
    if name.is_empty() {
        return "error: metric_record needs a 'name'".into();
    }
    let Some(value) = args.get("value").and_then(|v| v.as_f64()) else {
        return "error: metric_record needs a numeric 'value'".into();
    };
    let day = args
        .get("day")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-')
        .map(str::to_string)
        .unwrap_or_else(today);

    let unit = args.get("unit").and_then(|v| v.as_str()).map(str::trim).filter(|u| !u.is_empty());
    let note = args.get("note").and_then(|v| v.as_str()).map(str::trim).filter(|n| !n.is_empty());
    // Attributed to whoever recorded it, so a figure can be traced back to the agent
    // that read it — including when it turns out to be wrong. A turn run by a named
    // agent carries its id; a goal cycle carries it on the goal row instead.
    let source = ctx.persona_id.clone().or_else(|| {
        ctx.goal_id
            .as_deref()
            .and_then(|id| ctx.db.get_goal(id).ok().flatten())
            .and_then(|g| g.persona_id)
    });

    match ctx.db.upsert_metric(&ws_id, &name, &day, value, unit, note, source.as_deref()) {
        Ok(()) => format!(
            "Recorded {name} = {value}{} for {ws_name} on {day}.",
            unit.map(|u| format!(" {u}")).unwrap_or_default()
        ),
        Err(e) => format!("error recording {name}: {e}"),
    }
}

/// The number a metric command printed, or `None`.
///
/// The output must *begin* with the number, once leading whitespace is gone. Forgiving
/// about what follows it — `mysql -N` pads with tabs, `wc -l` leads with spaces, a money
/// total may carry a thousands separator or a trailing currency — and unforgiving about
/// what precedes it, because that is the difference between a figure and an error.
///
/// A looser reader that took the first number anywhere turned `ERROR 1045: Access
/// denied` into 1045, which would be filed as a day's revenue and believed. Nothing
/// about a number in the middle of a sentence says it is the measurement.
pub(crate) fn first_number(out: &str) -> Option<f64> {
    let rest = out.trim_start();
    let mut cur = String::new();
    for ch in rest.chars() {
        match ch {
            '0'..='9' => cur.push(ch),
            // Separators only count inside a number, so a stray "." or the "-" in a
            // date cannot start or extend one.
            '.' if cur.chars().any(|c| c.is_ascii_digit()) && !cur.contains('.') => cur.push(ch),
            ',' if cur.chars().any(|c| c.is_ascii_digit()) => continue,
            '-' if cur.is_empty() => cur.push('-'),
            _ => break,
        }
    }
    cur.chars()
        .any(|c| c.is_ascii_digit())
        .then(|| cur.trim_end_matches('.').parse().ok())?
}

async fn source_set(ctx: &ToolContext, args: &Value) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().to_lowercase();
    let vps = args.get("vps").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
    if name.is_empty() || vps.is_empty() || command.is_empty() {
        return "error: metric_source_set needs 'name', 'vps' and 'command'".into();
    }
    if !crate::ai::tools::is_target_allowed(&ctx.targets, &vps) {
        return format!(
            "error: {vps} is not one of the servers selected for this turn ({})",
            ctx.targets.join(", ")
        );
    }
    // A source runs unattended forever after one approval. Something that can change the
    // system is not a measurement, and this is the only moment anyone looks at it.
    if !crate::ai::safety::is_read_only(&command) {
        return format!(
            "error: refused — a metric source must be read-only, and {command:?} is not. \
             Reduce it to something that only reads and prints a number."
        );
    }

    let unit = args.get("unit").and_then(|v| v.as_str()).map(str::trim).filter(|u| !u.is_empty());
    let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

    if !enabled {
        return match ctx.db.upsert_metric_source(&ws_id, &name, &vps, &command, unit, false) {
            Ok(()) => format!("Stopped collecting {name} for {ws_name}."),
            Err(e) => format!("error: {e}"),
        };
    }

    let summary = format!(
        "Collect \"{name}\" for {ws_name} automatically.\n\n\
         On {vps}, run:\n  {command}\n\n\
         This runs unattended from now on, every time the numbers are collected. It is \
         read-only.",
    );
    if let Err(e) = crate::ai::safety::authorize(
        &ctx.app,
        &ctx.db,
        &ctx.approvals,
        "approve",
        &ctx.session_id,
        Some(&vps),
        &summary,
    )
    .await
    {
        return format!("not saved: {e}");
    }

    if let Err(e) = ctx.db.upsert_metric_source(&ws_id, &name, &vps, &command, unit, true) {
        return format!("error: {e}");
    }
    // Run it once now, so a command that does not work is found here rather than in
    // three days' worth of missing figures.
    match crate::ssh::command::run_vps_command(&ctx.db, &vps, &command).await {
        Ok(o) => match first_number(&o.stdout) {
            Some(v) => format!(
                "Saved, and it works: {name} = {v} right now. It will be collected from here on."
            ),
            None => format!(
                "Saved, but the command printed no number I could read:\n{}\nFix it with \
                 metric_source_set — as it stands nothing will be recorded.",
                o.stdout.chars().take(300).collect::<String>()
            ),
        },
        Err(e) => format!("Saved, but running it failed: {e}. Nothing will be recorded until it works."),
    }
}

async fn collect(ctx: &ToolContext, args: &Value) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let day = args
        .get("day")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|d| d.len() == 10)
        .map(str::to_string)
        .unwrap_or_else(today);

    let sources = ctx.db.list_metric_sources(&ws_id).unwrap_or_default();
    let live: Vec<_> = sources.into_iter().filter(|(_, _, _, _, on)| *on).collect();
    if live.is_empty() {
        return format!(
            "{ws_name} has no metric sources. Set one up with metric_source_set — until \
             then every figure has to be typed in by hand, which means it stops happening."
        );
    }

    let source_id = ctx.persona_id.clone().or_else(|| {
        ctx.goal_id
            .as_deref()
            .and_then(|id| ctx.db.get_goal(id).ok().flatten())
            .and_then(|g| g.persona_id)
    });

    let mut ok = Vec::new();
    let mut failed = Vec::new();
    for (name, vps, command, unit, _) in live {
        match crate::ssh::command::run_vps_command(&ctx.db, &vps, &command).await {
            Ok(o) => match first_number(&o.stdout) {
                Some(v) => {
                    match ctx.db.upsert_metric(
                        &ws_id,
                        &name,
                        &day,
                        v,
                        unit.as_deref(),
                        Some("collected automatically"),
                        source_id.as_deref(),
                    ) {
                        Ok(()) => ok.push(format!(
                            "{name} = {v}{}",
                            unit.map(|u| format!(" {u}")).unwrap_or_default()
                        )),
                        Err(e) => failed.push(format!("{name}: could not store ({e})")),
                    }
                }
                // Never recorded as zero: a source that printed an error would become a
                // day of no revenue, which reads as a collapse and is a lie.
                None => failed.push(format!("{name}: no number in the output")),
            },
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }

    let mut out = format!("{ws_name}, {day}: collected {} of {}.\n", ok.len(), ok.len() + failed.len());
    for line in &ok {
        out.push_str(&format!("- {line}\n"));
    }
    for line in &failed {
        out.push_str(&format!("- FAILED {line}\n"));
    }
    if !failed.is_empty() {
        out.push_str(
            "\nA source that fails records nothing rather than a zero. Fix it, or the \
             trend will have holes that look like a fall.\n",
        );
    }
    out
}

/// One metric's movement between two adjacent windows.
struct Movement {
    name: String,
    current: f64,
    previous: f64,
    days_with_data: i64,
    unit: Option<String>,
}

impl Movement {
    /// Percentage change, or `None` when there is nothing to compare against.
    ///
    /// Growth from zero is not "infinite percent" — it is a first sale, and saying so
    /// is more useful than a number that means nothing.
    fn change_pct(&self) -> Option<f64> {
        (self.previous.abs() > f64::EPSILON)
            .then(|| (self.current - self.previous) / self.previous * 100.0)
    }

    fn line(&self) -> String {
        let unit = self.unit.as_deref().map(|u| format!(" {u}")).unwrap_or_default();
        let verdict = match self.change_pct() {
            Some(p) if p > 0.5 => format!("up {p:.1}%"),
            Some(p) if p < -0.5 => format!("down {:.1}%", p.abs()),
            Some(_) => "flat".to_string(),
            None if self.current > 0.0 => "new — nothing recorded in the previous period".into(),
            None => "nothing recorded in either period".into(),
        };
        format!(
            "- {}: {:.2}{unit} this period vs {:.2}{unit} before — {verdict} ({} day(s) with data)",
            self.name, self.current, self.previous, self.days_with_data
        )
    }
}

/// The trend report for one project, without going through tool arguments.
///
/// Shared with `project_review`, so the weekly briefing and the direct question give the
/// same answer — a review that computed its own version of "revenue is down" would drift
/// from the one the user sees when they ask.
pub fn trend_for(
    ctx: &ToolContext,
    ws_id: &str,
    ws_name: &str,
    only: Option<&str>,
    days: i64,
) -> String {
    let names: Vec<String> = match only.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => vec![n.to_lowercase()],
        None => ctx.db.metric_names(ws_id).unwrap_or_default(),
    };
    if names.is_empty() {
        return format!(
            "{ws_name} has no figures recorded yet. Record them with metric_record — without \
             numbers there is no way to tell whether the work is helping.\n"
        );
    }

    // Half-open windows so a day belongs to exactly one of them.
    let today = chrono::Local::now().date_naive();
    let end = (today + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let mid = (today + chrono::Duration::days(1) - chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();
    let start = (today + chrono::Duration::days(1) - chrono::Duration::days(days * 2))
        .format("%Y-%m-%d")
        .to_string();

    let mut out = format!("{ws_name} — last {days} day(s) against the {days} before:\n");
    for name in names {
        let (current, days_with_data) =
            ctx.db.metric_total(ws_id, &name, &mid, &end).unwrap_or((0.0, 0));
        let (previous, _) = ctx.db.metric_total(ws_id, &name, &start, &mid).unwrap_or((0.0, 0));
        let unit = ctx
            .db
            .metric_series(ws_id, &name, &start)
            .ok()
            .and_then(|s| s.into_iter().find_map(|(_, _, u)| u));
        out.push_str(&Movement { name, current, previous, days_with_data, unit }.line());
        out.push('\n');
    }
    out
}

fn trend(ctx: &ToolContext, args: &Value) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let days = args.get("days").and_then(|v| v.as_i64()).unwrap_or(7).clamp(1, 365);
    let only = args.get("name").and_then(|v| v.as_str());
    let mut out = trend_for(ctx, &ws_id, &ws_name, only, days);
    out.push_str(
        "\nA metric that is down is a question, not a verdict: look at what changed in the \
         period (agent_activity, project_history) before deciding what to do about it.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(current: f64, previous: f64) -> Movement {
        Movement {
            name: "revenue".into(),
            current,
            previous,
            days_with_data: 7,
            unit: Some("EUR".into()),
        }
    }

    #[test]
    fn a_fall_is_reported_as_a_fall() {
        let line = m(1000.0, 1500.0).line();
        assert!(line.contains("down 33.3%"), "{line}");
    }

    #[test]
    fn a_rise_is_reported_as_a_rise() {
        assert!(m(1500.0, 1000.0).line().contains("up 50.0%"));
    }

    #[test]
    fn growth_from_nothing_is_not_infinite_percent() {
        // Dividing by a zero baseline produces inf, which renders as garbage and tells
        // the reader nothing. A first sale is worth saying in words.
        let line = m(250.0, 0.0).line();
        assert!(m(250.0, 0.0).change_pct().is_none());
        assert!(line.contains("new"), "{line}");
        assert!(!line.contains("inf") && !line.contains("NaN"), "{line}");
    }

    #[test]
    fn no_data_at_all_says_so_rather_than_reporting_flat() {
        // 0 vs 0 is "nobody recorded anything", not "sales held steady" — and the two
        // call for completely different responses.
        let line = m(0.0, 0.0).line();
        assert!(line.contains("nothing recorded in either period"), "{line}");
    }

    #[test]
    fn a_tiny_wobble_is_flat() {
        assert!(m(1000.0, 1000.2).line().contains("flat"));
    }

    #[test]
    fn a_number_is_found_however_the_command_printed_it() {
        // `mysql -N` pads with tabs, `wc -l` leads with spaces, a money total may carry
        // a thousands separator. Being strict would mean every source needing its own
        // `| sed`, which is how collecting numbers stops happening.
        assert_eq!(first_number("1234\n"), Some(1234.0));
        assert_eq!(first_number("\t1234.56\t\n"), Some(1234.56));
        assert_eq!(first_number("   42\n"), Some(42.0));
        assert_eq!(first_number("1,234.56"), Some(1234.56));
        assert_eq!(first_number("987 EUR"), Some(987.0));
        assert_eq!(first_number("-15"), Some(-15.0));
    }

    #[test]
    fn output_with_no_number_is_not_recorded_as_zero() {
        // A source that printed an error must not become a day with zero revenue: that
        // reads as a collapse, and would send a team chasing a fall that never happened.
        assert_eq!(first_number(""), None);
        // The one that mattered: a looser reader made this 1045 and filed it as a
        // day's revenue.
        assert_eq!(first_number("ERROR 1045: Access denied"), None);
        assert_eq!(first_number("total: 987"), None);
        assert_eq!(first_number("   \n\t"), None);
    }

    #[test]
    fn a_date_in_the_output_does_not_become_a_negative_number() {
        // `2026-08-30` starts with a perfectly good number, and the hyphen must not
        // turn the next field into -8. Whether that is the figure you wanted is the
        // command's problem; recording -8 would be this function's.
        assert_eq!(first_number("2026-08-30"), Some(2026.0));
    }

    #[test]
    fn defining_a_source_and_collecting_both_count_as_mutations() {
        // Defining one is a standing grant to run a command unattended; collecting
        // writes figures. Plan mode has to withhold both.
        assert!(tool_is_mutating("metric_source_set"));
        assert!(tool_is_mutating("metric_collect"));
        assert!(!tool_is_mutating("metric_trend"));
    }

    #[test]
    fn every_declared_tool_is_recognised() {
        for def in definitions() {
            assert!(is_metric_tool(&def.name), "{} is not routed", def.name);
        }
    }

    #[test]
    fn recording_a_figure_is_a_mutation_but_reading_one_is_not() {
        assert!(tool_is_mutating("metric_record"));
        assert!(!tool_is_mutating("metric_trend"));
    }
}

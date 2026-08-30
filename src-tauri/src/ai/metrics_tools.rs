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
    matches!(name, "metric_record" | "metric_trend")
}

/// Reading a trend changes nothing; recording a figure is a durable claim about the
/// business, so plan mode withholds it.
pub fn tool_is_mutating(name: &str) -> bool {
    name == "metric_record"
}

pub async fn dispatch(ctx: &ToolContext, name: &str, args: &Value) -> String {
    match name {
        "metric_record" => record(ctx, args),
        "metric_trend" => trend(ctx, args),
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

fn trend(ctx: &ToolContext, args: &Value) -> String {
    let (ws_id, ws_name) = match project_of(ctx, args) {
        Ok(v) => v,
        Err(e) => return format!("error: {e}"),
    };
    let days = args.get("days").and_then(|v| v.as_i64()).unwrap_or(7).clamp(1, 365);

    let names: Vec<String> = match args.get("name").and_then(|v| v.as_str()).map(str::trim) {
        Some(n) if !n.is_empty() => vec![n.to_lowercase()],
        _ => ctx.db.metric_names(&ws_id).unwrap_or_default(),
    };
    if names.is_empty() {
        return format!(
            "{ws_name} has no figures recorded yet. Record them with metric_record — without \
             numbers there is no way to tell whether the work is helping."
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
            ctx.db.metric_total(&ws_id, &name, &mid, &end).unwrap_or((0.0, 0));
        let (previous, _) = ctx.db.metric_total(&ws_id, &name, &start, &mid).unwrap_or((0.0, 0));
        let unit = ctx
            .db
            .metric_series(&ws_id, &name, &start)
            .ok()
            .and_then(|s| s.into_iter().find_map(|(_, _, u)| u));
        out.push_str(&Movement { name, current, previous, days_with_data, unit }.line());
        out.push('\n');
    }
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

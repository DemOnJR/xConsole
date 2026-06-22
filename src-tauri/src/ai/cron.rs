//! Lightweight cron scheduler. Ticks on a fixed interval, evaluates each enabled
//! job's schedule, and runs due jobs — either a raw command (per target, honoring
//! the safety mode) or an agent prompt (full agent loop). One scheduler, reusing
//! the same exec path, agent loop, and safety gate as everything else.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, TimeZone, Utc, Weekday};
use dashmap::DashSet;
use tauri::{AppHandle, Emitter};

use crate::ai::provider::{ChatMessage, StreamEvent};
use crate::ai::safety::{self, ApprovalRegistry};
use crate::ai::tools::ToolContext;
use crate::ai::{agent, AgentHome};
use crate::ssh::SessionManager;
use crate::storage::{models::CronJob, Db};

/// How often the scheduler wakes up to check for due jobs.
const TICK: StdDuration = StdDuration::from_secs(30);

/// Job ids currently executing (prevents overlapping runs). Shared app state.
#[derive(Clone, Default)]
pub struct CronRunning {
    pub jobs: Arc<DashSet<String>>,
}

/// Shared handles the scheduler needs. All cheap to clone.
#[derive(Clone)]
pub struct CronContext {
    pub app: AppHandle,
    pub db: Db,
    pub sessions: SessionManager,
    pub home: AgentHome,
    pub approvals: ApprovalRegistry,
    pub running: CronRunning,
}

/// Clears a job from the in-flight set when its run finishes.
struct RunGuard {
    running: CronRunning,
    id: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.running.jobs.remove(&self.id);
    }
}

/// A parsed schedule. Supported grammar (kept simple and user-friendly):
/// - `@every <n>(s|m|h|d)`   e.g. `@every 5m`
/// - `@hourly`
/// - `@daily HH:MM`          (local time)
/// - `@weekly <weekday> HH:MM`
enum Schedule {
    Every(Duration),
    Daily { h: u32, m: u32 },
    Weekly { wd: Weekday, h: u32, m: u32 },
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s.to_lowercase().as_str() {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    Some((h.trim().parse().ok()?, m.trim().parse().ok()?))
}

fn parse_schedule(spec: &str) -> Option<Schedule> {
    let spec = spec.trim();
    if let Some(rest) = spec.strip_prefix("@every") {
        let rest = rest.trim();
        let (num, unit) = rest.split_at(rest.find(|c: char| c.is_alphabetic())?);
        let n: i64 = num.trim().parse().ok()?;
        let dur = match unit.trim() {
            "s" => Duration::seconds(n),
            "m" => Duration::minutes(n),
            "h" => Duration::hours(n),
            "d" => Duration::days(n),
            _ => return None,
        };
        return Some(Schedule::Every(dur));
    }
    if spec == "@hourly" {
        return Some(Schedule::Every(Duration::hours(1)));
    }
    if let Some(rest) = spec.strip_prefix("@daily") {
        let (h, m) = parse_hhmm(rest.trim())?;
        return Some(Schedule::Daily { h, m });
    }
    if let Some(rest) = spec.strip_prefix("@weekly") {
        let mut parts = rest.trim().split_whitespace();
        let wd = parse_weekday(parts.next()?)?;
        let (h, m) = parse_hhmm(parts.next()?)?;
        return Some(Schedule::Weekly { wd, h, m });
    }
    None
}

/// Parse SQLite's `datetime('now')` output (UTC, "%Y-%m-%d %H:%M:%S").
fn parse_db_time(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|ndt| Utc.from_utc_datetime(&ndt))
}

fn is_due(schedule: &Schedule, last_run: Option<DateTime<Utc>>) -> bool {
    match schedule {
        Schedule::Every(dur) => match last_run {
            Some(lr) => Utc::now() - lr >= *dur,
            None => true,
        },
        Schedule::Daily { h, m } => {
            let now = Local::now();
            let target = now.date_naive().and_hms_opt(*h, *m, 0);
            let Some(target) = target.and_then(|t| Local.from_local_datetime(&t).single()) else {
                return false;
            };
            if now < target {
                return false;
            }
            match last_run.map(|lr| lr.with_timezone(&Local)) {
                Some(lr) => lr < target,
                None => true,
            }
        }
        Schedule::Weekly { wd, h, m } => {
            let now = Local::now();
            if now.weekday() != *wd {
                return false;
            }
            let Some(target) = now
                .date_naive()
                .and_hms_opt(*h, *m, 0)
                .and_then(|t| Local.from_local_datetime(&t).single())
            else {
                return false;
            };
            if now < target {
                return false;
            }
            match last_run.map(|lr| lr.with_timezone(&Local)) {
                Some(lr) => lr < target,
                None => true,
            }
        }
    }
}

/// Spawn the scheduler on the Tauri async runtime.
pub fn spawn(ctx: CronContext) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            let jobs = match ctx.db.list_cron_jobs() {
                Ok(j) => j,
                Err(_) => continue,
            };
            for job in jobs {
                if !job.enabled {
                    continue;
                }
                let Some(schedule) = parse_schedule(&job.schedule) else {
                    continue;
                };
                let last = job.last_run.as_deref().and_then(parse_db_time);
                if is_due(&schedule, last) {
                    let ctx = ctx.clone();
                    tauri::async_runtime::spawn(async move {
                        run_job(&ctx, &job).await;
                    });
                }
            }
        }
    });
}

/// Run a single job now. Public so the UI can trigger "run now".
pub async fn run_job(ctx: &CronContext, job: &CronJob) {
    if !ctx.running.jobs.insert(job.id.clone()) {
        let event = format!("ai://cron/{}", job.id);
        let _ = ctx.app.emit(
            &event,
            StreamEvent::Error("job already running".into()),
        );
        return;
    }
    let _guard = RunGuard {
        running: ctx.running.clone(),
        id: job.id.clone(),
    };

    let event = format!("ai://cron/{}", job.id);
    let _ = ctx
        .app
        .emit(&event, StreamEvent::Status(format!("running '{}'", job.name)));

    let targets: Vec<String> = job
        .targets_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let result: Result<(), String> = match job.kind.as_str() {
        "command" => run_command_job(ctx, job, &targets, &event).await,
        "prompt" => run_prompt_job(ctx, job, targets, &event).await,
        other => Err(format!("unknown job kind '{other}'")),
    };

    let status = match &result {
        Ok(()) => "ok".to_string(),
        Err(e) => {
            let _ = ctx.app.emit(&event, StreamEvent::Error(e.clone()));
            format!("error: {e}")
        }
    };
    let _ = ctx.db.mark_cron_run(&job.id, &status);
    let _ = ctx.app.emit(&event, StreamEvent::Done);
}

async fn run_command_job(
    ctx: &CronContext,
    job: &CronJob,
    targets: &[String],
    event: &str,
) -> Result<(), String> {
    if targets.is_empty() {
        return Err("no targets configured".into());
    }
    let global = ctx
        .db
        .get_setting("agent.safety_mode")
        .ok()
        .flatten()
        .unwrap_or_else(|| "approve".to_string());

    for vps_id in targets {
        let mode = safety::effective_mode(&ctx.db, &global, vps_id);
        safety::authorize(
            &ctx.app,
            &ctx.db,
            &ctx.approvals,
            &mode,
            &format!("cron:{}", job.id),
            Some(vps_id),
            &job.payload,
        )
        .await?;

        let out = ctx.sessions.run_command(vps_id, &job.payload).await?;
        let _ = ctx.app.emit(
            event,
            StreamEvent::ToolResult {
                id: vps_id.clone(),
                output: format!(
                    "[{vps_id}] exit {}\n{}\n{}",
                    out.exit_code,
                    out.stdout.trim_end(),
                    out.stderr.trim_end()
                ),
            },
        );
    }
    Ok(())
}

async fn run_prompt_job(
    ctx: &CronContext,
    job: &CronJob,
    targets: Vec<String>,
    event: &str,
) -> Result<(), String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let app = ctx.app.clone();
    let event_owned = event.to_string();
    let forward = tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = app.emit(&event_owned, ev);
        }
    });

    let global = ctx
        .db
        .get_setting("agent.safety_mode")
        .ok()
        .flatten()
        .unwrap_or_else(|| "approve".to_string());

    let tc = ToolContext {
        app: ctx.app.clone(),
        db: ctx.db.clone(),
        sessions: ctx.sessions.clone(),
        home: ctx.home.clone(),
        approvals: ctx.approvals.clone(),
        session_id: format!("cron:{}", job.id),
        targets,
        safety: global,
    };

    let messages = vec![ChatMessage::user(job.payload.clone())];
    let result = agent::run_turn(&tc, None, messages, &tx).await.map(|_| ());
    drop(tc);
    let _ = forward.await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_every_schedule() {
        assert!(matches!(
            parse_schedule("@every 5m"),
            Some(Schedule::Every(_))
        ));
        assert!(matches!(
            parse_schedule("@every 1h"),
            Some(Schedule::Every(_))
        ));
    }

    #[test]
    fn parse_daily_and_weekly() {
        assert!(matches!(
            parse_schedule("@daily 09:30"),
            Some(Schedule::Daily { h: 9, m: 30 })
        ));
        assert!(matches!(
            parse_schedule("@weekly mon 08:00"),
            Some(Schedule::Weekly { h: 8, m: 0, .. })
        ));
    }

    #[test]
    fn parse_invalid_schedule() {
        assert!(parse_schedule("not a schedule").is_none());
        assert!(parse_schedule("@every").is_none());
    }
}

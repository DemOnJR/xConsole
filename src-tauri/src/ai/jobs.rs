//! Long-running commands the agent starts and then stops waiting for.
//!
//! Every command the agent ran went through `run_vps_command`, which has a hard
//! 120-second ceiling. Anything real — a build, `apt upgrade`, a docker pull, an
//! rsync of any size — hit that ceiling, failed, and left the agent with no result
//! and no way to recover the work. So the agent had to hand those tasks back to the
//! user, which is exactly the interruption we want to remove.
//!
//! A job is started detached on the remote host and its output is redirected to a
//! file there. The **remote host is the only source of truth**: status comes from
//! the pid file, output from the log file. Nothing is tracked locally, so a job
//! survives the turn that started it, the session, and a restart of xConsole — and
//! two xConsole instances looking at the same server see the same jobs.

use crate::ssh::remote_ops::shell_quote;

/// Where job state lives on the remote host.
///
/// `/tmp` because it is world-writable, present on every POSIX host, and cleaned by
/// the OS — job logs are progress output, not records worth keeping past a reboot.
const JOB_DIR: &str = "/tmp/.xconsole-jobs";

/// How much of a log to hand back by default. Enough to see what a build is doing
/// without pushing the rest of the conversation out of the context window.
pub const DEFAULT_TAIL_LINES: u32 = 40;

/// Job ids are generated here and interpolated into remote paths, so they must not
/// be able to carry anything a shell or a path would interpret.
pub fn is_valid_job_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A short, collision-resistant, shell-safe job id.
pub fn new_job_id() -> String {
    // The uuid's first block is 8 hex chars: unambiguous in a log line, and short
    // enough that the model can repeat it back without transcription errors.
    let raw = uuid::Uuid::new_v4().to_string();
    format!("job-{}", &raw[..8])
}

/// Shell to start `command` detached, writing output and pid under [`JOB_DIR`].
///
/// `setsid` (falling back to plain background) detaches the process from the SSH
/// session, so it keeps running after the exec channel closes — without it the job
/// dies the moment the command that started it returns. `exit_code` is recorded by a
/// wrapper so a finished job can report how it ended, not merely that it stopped.
pub fn start_script(job_id: &str, command: &str) -> String {
    let dir = JOB_DIR;
    let log = format!("{dir}/{job_id}.log");
    let pid = format!("{dir}/{job_id}.pid");
    let exit = format!("{dir}/{job_id}.exit");
    let meta = format!("{dir}/{job_id}.cmd");
    // The user command is passed to `sh -c` as a single quoted argument, so nothing
    // in it can terminate the wrapper and run as a separate statement.
    let quoted = shell_quote(command);
    format!(
        "mkdir -p {dir} && \
         printf '%s' {quoted} > {meta} && \
         {{ setsid sh -c {quoted} > {log} 2>&1 < /dev/null || sh -c {quoted} > {log} 2>&1 < /dev/null ; \
            printf '%s' \"$?\" > {exit} ; }} & \
         printf '%s' \"$!\" > {pid} ; \
         echo started"
    )
}

/// Shell to report one job's status: pid, whether it is still alive, exit code if
/// it finished, the command that was run, and the tail of its output.
///
/// One round trip rather than four: an agent checking on a build should not cost
/// four SSH connections.
pub fn status_script(job_id: &str, tail_lines: u32) -> String {
    let dir = JOB_DIR;
    let log = format!("{dir}/{job_id}.log");
    let pid = format!("{dir}/{job_id}.pid");
    let exit = format!("{dir}/{job_id}.exit");
    let meta = format!("{dir}/{job_id}.cmd");
    let n = tail_lines.clamp(1, 500);
    format!(
        "if [ ! -f {pid} ]; then echo 'STATE=unknown'; exit 0; fi; \
         P=$(cat {pid} 2>/dev/null); \
         if [ -f {exit} ]; then echo \"STATE=finished\"; echo \"EXIT=$(cat {exit})\"; \
         elif kill -0 \"$P\" 2>/dev/null; then echo 'STATE=running'; \
         else echo 'STATE=stopped'; fi; \
         echo \"PID=$P\"; \
         echo \"COMMAND=$(cat {meta} 2>/dev/null)\"; \
         echo '--- output (last {n} lines) ---'; \
         tail -n {n} {log} 2>/dev/null || echo '(no output yet)'"
    )
}

/// Shell to list every job on the host, newest first, one `id state command` per line.
pub fn list_script() -> String {
    let dir = JOB_DIR;
    format!(
        "if [ ! -d {dir} ]; then echo '(no jobs)'; exit 0; fi; \
         found=0; \
         for f in $(ls -1t {dir}/*.pid 2>/dev/null); do \
           found=1; \
           id=$(basename \"$f\" .pid); \
           P=$(cat \"$f\" 2>/dev/null); \
           if [ -f {dir}/$id.exit ]; then S=\"finished(exit $(cat {dir}/$id.exit))\"; \
           elif kill -0 \"$P\" 2>/dev/null; then S=running; \
           else S=stopped; fi; \
           echo \"$id  $S  $(cut -c1-100 {dir}/$id.cmd 2>/dev/null)\"; \
         done; \
         [ \"$found\" = 0 ] && echo '(no jobs)'; \
         exit 0"
    )
}

/// Shell to stop a running job.
///
/// Signals the whole process group (`-PID`) because a job is usually a shell that
/// spawned the real work — killing only the wrapper would orphan the build and leave
/// it running with nothing tracking it. Falls back to the bare pid for hosts where
/// the job never got its own group. TERM first so the process can clean up; KILL
/// after a grace period for anything that ignores it.
pub fn kill_script(job_id: &str) -> String {
    let dir = JOB_DIR;
    let pid = format!("{dir}/{job_id}.pid");
    format!(
        "if [ ! -f {pid} ]; then echo 'unknown job'; exit 0; fi; \
         P=$(cat {pid}); \
         kill -TERM -\"$P\" 2>/dev/null || kill -TERM \"$P\" 2>/dev/null || true; \
         sleep 2; \
         if kill -0 \"$P\" 2>/dev/null; then \
           kill -KILL -\"$P\" 2>/dev/null || kill -KILL \"$P\" 2>/dev/null || true; \
           echo 'killed (SIGKILL)'; \
         else echo 'stopped (SIGTERM)'; fi"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_are_shell_and_path_safe() {
        assert!(is_valid_job_id("job-1a2b3c4d"));
        assert!(is_valid_job_id("a_b-C9"));
        // Anything that could break out of a path or a shell word is refused.
        for bad in [
            "", "job 1", "job;rm", "../etc", "job/../x", "job$(id)", "job`id`",
            "job\nrm", "job'x", "job\"x", "job|x", "job&x", "job*",
        ] {
            assert!(!is_valid_job_id(bad), "should reject {bad:?}");
        }
        assert!(!is_valid_job_id(&"a".repeat(65)));
    }

    #[test]
    fn generated_ids_are_valid_and_distinct() {
        let a = new_job_id();
        let b = new_job_id();
        assert!(is_valid_job_id(&a), "{a}");
        assert!(is_valid_job_id(&b), "{b}");
        assert_ne!(a, b);
    }

    #[test]
    fn a_command_cannot_escape_its_quoting() {
        // The classic break-out: close the quote, run something else.
        let script = start_script("job-1", "echo hi'; rm -rf / ;'");
        assert!(!script.contains("; rm -rf / ;'\n"), "{script}");
        // POSIX close-escape-reopen keeps it one argument.
        assert!(script.contains(r#"'echo hi'\''; rm -rf / ;'\'''"#), "{script}");
    }

    #[test]
    fn a_command_with_a_quote_stays_one_argument() {
        let script = start_script("job-1", "echo it's fine");
        assert!(script.contains(r#"'echo it'\''s fine'"#), "{script}");
    }

    #[test]
    fn start_detaches_and_records_pid_and_exit_code() {
        let s = start_script("job-abc", "make");
        // Detached, so the job outlives the SSH channel that started it.
        assert!(s.contains("setsid"), "{s}");
        assert!(s.contains("< /dev/null"), "{s}");
        // Both halves of "how did it end" are captured.
        assert!(s.contains("/tmp/.xconsole-jobs/job-abc.pid"), "{s}");
        assert!(s.contains("/tmp/.xconsole-jobs/job-abc.exit"), "{s}");
        assert!(s.contains("/tmp/.xconsole-jobs/job-abc.log"), "{s}");
    }

    #[test]
    fn status_distinguishes_running_from_finished_from_unknown() {
        let s = status_script("job-abc", 10);
        assert!(s.contains("STATE=running"), "{s}");
        assert!(s.contains("STATE=finished"), "{s}");
        assert!(s.contains("STATE=unknown"), "{s}");
        assert!(s.contains("tail -n 10"), "{s}");
    }

    #[test]
    fn status_tail_is_clamped_to_a_sane_range() {
        // 0 lines would return nothing useful; a huge tail would flood the context.
        assert!(status_script("j", 0).contains("tail -n 1 "), "{}", status_script("j", 0));
        assert!(
            status_script("j", 100_000).contains("tail -n 500 "),
            "{}",
            status_script("j", 100_000)
        );
    }

    #[test]
    fn kill_targets_the_process_group_then_escalates() {
        let s = kill_script("job-abc");
        // The group, so a build spawned by the wrapper shell dies with it.
        assert!(s.contains("kill -TERM -\"$P\""), "{s}");
        assert!(s.contains("kill -KILL -\"$P\""), "{s}");
        // TERM is given a chance before KILL.
        let term = s.find("-TERM").unwrap();
        let kill = s.find("-KILL").unwrap();
        assert!(term < kill, "{s}");
    }
}

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
///
/// Two details make starting a job return immediately, and neither is cosmetic.
///
/// The group is redirected — `{ … } > /dev/null 2>&1 < /dev/null &` — because otherwise
/// the background subshell holds the caller's stdout, the SSH channel never reaches EOF
/// and a local `.output()` never returns.
///
/// And the setup steps are separated by `;`, not `&&`. With `&&` the trailing `&`
/// backgrounds the *whole and-list*, so the subshell running `mkdir` sits in the
/// background waiting for the group with the original stdout still open — and holds the
/// pipe just as surely. Found by timing it: starting a `sleep 60` job took sixty
/// seconds, so a two-hour build blocked the turn until the timeout fired and was then
/// reported as a failure, for a job running perfectly well. Returning is the one thing
/// backgrounding has to do.
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
        "mkdir -p {dir} ; \
         printf '%s' {quoted} > {meta} ; \
         {{ setsid sh -c {quoted} > {log} 2>&1 < /dev/null || sh -c {quoted} > {log} 2>&1 < /dev/null ; \
            printf '%s' \"$?\" > {exit} ; }} > /dev/null 2>&1 < /dev/null & \
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

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
//
// The same job, on a machine with no `sh`, no `setsid` and no signals. The file
// layout and the printed contract (`STATE=`, `EXIT=`, `PID=`, `COMMAND=`) are
// identical on purpose: only the script differs, so everything that reads a job's
// status stays one implementation.

/// Where job state lives on Windows. `%TEMP%` for the same reasons as `/tmp`.
const WIN_JOB_DIR: &str = "$env:TEMP\\.xconsole-jobs";

/// Quote a string as a PowerShell single-quoted literal.
///
/// Inside single quotes PowerShell interprets nothing at all — no `$`, no backtick,
/// no subexpression — so doubling the single quote is the whole escape. That matters
/// more here than usual: the command comes from the model, and it is about to be
/// written into a script that runs detached with nobody watching.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// PowerShell's `-EncodedCommand` payload: UTF-16LE, then base64.
///
/// Used so the inner script never has to survive a second round of quoting on the
/// command line. Every attempt to escape a script *through* an argument list is a
/// bug waiting for a command containing a quote, and agent commands contain quotes.
pub fn ps_encode(script: &str) -> String {
    use base64::Engine;
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// PowerShell to start `command` detached, with the same files the POSIX version writes.
pub fn start_script_windows(job_id: &str, command: &str) -> String {
    let dir = WIN_JOB_DIR;
    let log = format!("{dir}\\{job_id}.log");
    let pid = format!("{dir}\\{job_id}.pid");
    let exit = format!("{dir}\\{job_id}.exit");
    let meta = format!("{dir}\\{job_id}.cmd");

    // The detached half. `Invoke-Expression` gives the same "run this the way the user
    // typed it" semantics as `sh -c`. The exit code is written last, and its presence is
    // what `status` reads as "finished" — so a job that is killed mid-run correctly
    // reports as stopped rather than finished.
    let inner = format!(
        "$ErrorActionPreference='Continue';          $global:LASTEXITCODE=0;          try {{ Invoke-Expression {cmd} *>&1 | Out-File -LiteralPath \"{log}\" -Encoding utf8 }}          catch {{ $_ | Out-File -LiteralPath \"{log}\" -Append -Encoding utf8; $global:LASTEXITCODE=1 }};          $c = if ($null -eq $LASTEXITCODE) {{ 0 }} else {{ $LASTEXITCODE }};          Set-Content -LiteralPath \"{exit}\" -Value $c",
        cmd = ps_quote(command),
    );

    format!(
        "$d=\"{dir}\"; New-Item -ItemType Directory -Force -Path $d | Out-Null;          Set-Content -LiteralPath \"{meta}\" -Value {cmd};          $p = Start-Process powershell -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{enc}'               -WindowStyle Hidden -PassThru;          Set-Content -LiteralPath \"{pid}\" -Value $p.Id;          Write-Output 'started'",
        cmd = ps_quote(command),
        enc = ps_encode(&inner),
    )
}

/// PowerShell reporting one job, in the same shape the POSIX script prints.
pub fn status_script_windows(job_id: &str, tail_lines: u32) -> String {
    let dir = WIN_JOB_DIR;
    let n = tail_lines.clamp(1, 500);
    format!(
        "$d=\"{dir}\"; $id='{job_id}';          if (-not (Test-Path \"$d\\$id.pid\")) {{ Write-Output 'STATE=unknown'; exit 0 }};          $P = (Get-Content -LiteralPath \"$d\\$id.pid\" -Raw).Trim();          if (Test-Path \"$d\\$id.exit\") {{             Write-Output 'STATE=finished';             Write-Output \"EXIT=$((Get-Content -LiteralPath \"$d\\$id.exit\" -Raw).Trim())\" }}          elseif (Get-Process -Id $P -ErrorAction SilentlyContinue) {{ Write-Output 'STATE=running' }}          else {{ Write-Output 'STATE=stopped' }};          Write-Output \"PID=$P\";          Write-Output \"COMMAND=$(Get-Content -LiteralPath \"$d\\$id.cmd\" -Raw -ErrorAction SilentlyContinue)\";          Write-Output '--- output (last {n} lines) ---';          if (Test-Path \"$d\\$id.log\") {{ Get-Content -LiteralPath \"$d\\$id.log\" -Tail {n} }}          else {{ Write-Output '(no output yet)' }}"
    )
}

/// PowerShell listing every job, newest first, in the same `id state command` shape.
pub fn list_script_windows() -> String {
    let dir = WIN_JOB_DIR;
    format!(
        "$d=\"{dir}\";          if (-not (Test-Path $d)) {{ Write-Output '(no jobs)'; exit 0 }};          $f = Get-ChildItem -LiteralPath $d -Filter *.pid -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending;          if (-not $f) {{ Write-Output '(no jobs)'; exit 0 }};          foreach ($x in $f) {{            $id = $x.BaseName;            $P = (Get-Content -LiteralPath $x.FullName -Raw).Trim();            if (Test-Path \"$d\\$id.exit\") {{ $S = \"finished(exit $((Get-Content -LiteralPath \"$d\\$id.exit\" -Raw).Trim()))\" }}            elseif (Get-Process -Id $P -ErrorAction SilentlyContinue) {{ $S = 'running' }}            else {{ $S = 'stopped' }};            $c = (Get-Content -LiteralPath \"$d\\$id.cmd\" -Raw -ErrorAction SilentlyContinue);            if ($c.Length -gt 100) {{ $c = $c.Substring(0,100) }};            Write-Output \"$id  $S  $c\" }}"
    )
}

/// PowerShell stopping a running job, with its children.
///
/// `Stop-Process -Force` on the wrapper alone would orphan the real work, exactly as
/// killing only the shell does on POSIX — so the tree goes too.
pub fn kill_script_windows(job_id: &str) -> String {
    let dir = WIN_JOB_DIR;
    format!(
        "$d=\"{dir}\"; $id='{job_id}';          if (-not (Test-Path \"$d\\$id.pid\")) {{ Write-Output 'unknown job'; exit 0 }};          $P = (Get-Content -LiteralPath \"$d\\$id.pid\" -Raw).Trim();          $proc = Get-Process -Id $P -ErrorAction SilentlyContinue;          if (-not $proc) {{ Write-Output 'already stopped'; exit 0 }};          Get-CimInstance Win32_Process -Filter \"ParentProcessId=$P\" -ErrorAction SilentlyContinue |            ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }};          Stop-Process -Id $P -Force -ErrorAction SilentlyContinue;          Write-Output 'killed'"
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
    #[test]
    fn a_powershell_literal_survives_a_quote_in_the_command() {
        // The command comes from the model and is written into a script that then runs
        // detached with nobody watching. Inside single quotes PowerShell interprets
        // nothing at all, so doubling the quote is the whole escape.
        assert_eq!(ps_quote("echo hi"), "'echo hi'");
        assert_eq!(ps_quote("echo 'hi'"), "'echo ''hi'''");
        // Characters that would otherwise expand are inert in a literal.
        let q = ps_quote("$env:PATH `whoami` $(calc)");
        assert!(q.starts_with('\'') && q.ends_with('\''), "{q}");
        assert!(q.contains("$env:PATH"), "{q}");
    }

    #[test]
    fn the_encoded_command_is_utf16le_base64() {
        // PowerShell's -EncodedCommand is UTF-16LE. Encoding it as UTF-8 produces a
        // script that decodes to mojibake and then silently does nothing at all.
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(ps_encode("Ab"))
            .expect("valid base64");
        assert_eq!(raw, vec![b'A', 0, b'b', 0]);
    }

    #[test]
    fn a_quote_in_the_command_cannot_break_out_of_the_windows_starter() {
        // The case worth getting right: a command containing a quote, reaching a script
        // that launches a detached process.
        let script = start_script_windows("job-abcd1234", "echo 'a'; Stop-Computer");
        assert!(script.contains("'echo ''a''; Stop-Computer'"), "{script}");
        // The detached half rides as base64, so it needs no escaping at all.
        assert!(script.contains("-EncodedCommand"), "{script}");
    }

    #[test]
    fn both_platforms_print_the_same_contract() {
        // Only the script differs; everything that reads a job's status is one
        // implementation, and it would break silently if the keys drifted apart.
        let posix = status_script("job-abcd1234", 40);
        let win = status_script_windows("job-abcd1234", 40);
        for key in ["STATE=", "PID=", "COMMAND=", "--- output (last 40 lines) ---"] {
            assert!(posix.contains(key), "posix is missing {key}");
            assert!(win.contains(key), "windows is missing {key}");
        }
        for s in [&posix, &win] {
            assert!(s.contains("STATE=unknown"), "no unknown state");
            assert!(s.contains("STATE=running"), "no running state");
        }
    }

    #[test]
    fn the_windows_kill_takes_the_children_too() {
        // Stopping only the wrapper orphans the real work, exactly as killing only the
        // shell does on POSIX.
        let k = kill_script_windows("job-abcd1234");
        assert!(k.contains("ParentProcessId"), "children are not killed: {k}");
        assert!(k.contains("Stop-Process"), "{k}");
        assert!(k.contains("unknown job"), "an unknown id should say so: {k}");
    }
    /// Run a shell command and return everything it printed.
    #[cfg(unix)]
    fn sh(cmd: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .expect("shell runs");
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    /// Wait for a job to reach a state, rather than sleeping a guessed interval.
    ///
    /// The first version of these tests slept a fixed two seconds and asserted the job
    /// was still running. On a loaded machine it had already finished, so a passing
    /// implementation failed its own test — which is how a flaky test teaches people to
    /// re-run rather than to look.
    #[cfg(unix)]
    fn wait_for(id: &str, needle: &str, secs: u64) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let s = sh(&status_script(id, 10));
            if s.contains(needle) || std::time::Instant::now() > deadline {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }

    #[cfg(unix)]
    fn cleanup(id: &str) {
        let _ = sh(&format!("rm -f /tmp/.xconsole-jobs/{id}.*"));
    }

    #[test]
    #[cfg(unix)]
    fn starting_a_job_returns_at_once_rather_than_waiting_for_it() {
        // The bug this was written to catch, and it is the whole point of the feature:
        // redirecting only the inner command left the background subshell holding the
        // caller's stdout, so the SSH channel never reached EOF and `.output()` never
        // returned. Starting a two-hour build blocked the turn until the timeout fired
        // and then reported a failure, for a job that was running perfectly well.
        let id = new_job_id();
        let began = std::time::Instant::now();
        let out = sh(&start_script(&id, "sleep 60"));
        let took = began.elapsed();

        assert!(out.contains("started"), "did not start: {out}");
        assert!(
            took < std::time::Duration::from_secs(5),
            "starting the job blocked for {took:?} — it should return at once"
        );
        // And the job really is running, so this is not "returned fast because it died".
        assert!(wait_for(&id, "STATE=running", 5).contains("STATE=running"));

        let _ = sh(&kill_script(&id));
        cleanup(&id);
    }

    #[test]
    #[cfg(unix)]
    fn a_job_outlives_its_starter_and_reports_how_it_ended() {
        // Read back rather than assumed: the exit code, the captured output, and the
        // fact that it kept going after the shell that launched it returned. Each has
        // been a silent failure somewhere — a job that died with its parent, a status
        // that could not tell running from finished, an exit code nobody wrote.
        let id = new_job_id();
        assert!(sh(&start_script(&id, "echo done-working; exit 7")).contains("started"));

        let done = wait_for(&id, "STATE=finished", 15);
        assert!(done.contains("STATE=finished"), "never finished: {done}");
        assert!(done.contains("EXIT=7"), "wrong or missing exit code: {done}");
        assert!(done.contains("done-working"), "output was not captured: {done}");
        assert!(done.contains("echo done-working"), "command not recorded: {done}");
        cleanup(&id);
    }

    #[test]
    #[cfg(unix)]
    fn a_running_job_is_listed_and_can_be_killed() {
        // Long enough that it cannot finish underneath the assertions, which is what
        // made the first attempt flaky.
        let id = new_job_id();
        assert!(sh(&start_script(&id, "sleep 120")).contains("started"));

        let running = wait_for(&id, "STATE=running", 10);
        assert!(running.contains("STATE=running"), "not running: {running}");
        assert!(list_script_contains(&id), "missing from the listing");

        let killed = sh(&kill_script(&id));
        assert!(killed.contains("stopped") || killed.contains("killed"), "{killed}");
        assert!(!sh(&status_script(&id, 5)).contains("STATE=running"), "still running after kill");
        cleanup(&id);
    }

    #[cfg(unix)]
    fn list_script_contains(id: &str) -> bool {
        sh(&list_script()).contains(id)
    }

    #[test]
    fn an_unknown_job_is_unknown_on_both_platforms() {
        // Never mistaken for "finished with no output", which would have an agent
        // conclude that work it never started had completed.
        for script in [status_script("job-deadbeef", 5), status_script_windows("job-deadbeef", 5)] {
            assert!(script.contains("STATE=unknown"), "{script}");
        }
    }
}

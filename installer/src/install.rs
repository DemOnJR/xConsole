// xConsole setup — Hermes-style "clone + compile on the user's PC".
//
// The installer is a tiny Tauri app. On "Install" it:
//   1. ensures the build toolchain (Git, Rust GNU, MinGW, Node+pnpm) — using the
//      system copy if present, otherwise downloading a portable one under the app dir,
//   2. clones (or pulls) https://github.com/DemOnJR/xConsole,
//   3. compiles xConsole from source via the GNU toolchain (no MSVC needed),
//   4. copies the built app + makes shortcuts + registers an uninstaller.
// The whole thing is idempotent, so re-running it = update (pull latest + rebuild).
//
// Everything lives per-user under %LOCALAPPDATA%\xConsole (no admin / UAC).
//
// Progress is reported through a `Reporter`, which fans out to (a) the Tauri UI via
// `install://*` events and (b) a persistent %LOCALAPPDATA%\xConsole\install.log.
// `xConsole-Setup.exe --install` runs the whole thing headlessly (for debugging),
// writing only to install.log.

use sha2::{Digest, Sha256};
use std::io::{BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const REPO_URL: &str = "https://github.com/DemOnJR/xConsole";
const GNU_TOOLCHAIN: &str = "stable-x86_64-pc-windows-gnu";

// Portable toolchain downloads (only used when the tool isn't already on PATH).
//
// SUPPLY-CHAIN INTEGRITY: every artifact below is verified BEFORE it is unzipped or
// executed. TLS only protects transport; it does not stop a maliciously-replaced
// GitHub/CDN release asset, and whatever we download gets compiled straight into the
// user's xconsole.exe (which holds their SSH credentials). Defenses:
//   * Version-pinned ZIPs   -> SHA-256 pinned here, checked with verify_sha256().
//   * rustup-init.exe       -> rolling AND not Authenticode-signed, so we pin a
//                              specific versioned archive build + its SHA-256.
//   * WebView2 bootstrapper -> rolling but Microsoft-signed, so we verify its
//                              Authenticode signature with verify_authenticode().
// To bump any pinned hash: change the URL, download the asset, recompute with
// PowerShell `Get-FileHash -Algorithm SHA256 <file>`, and paste the digest below.

// rustup-init.exe is NOT Authenticode-signed and https://win.rustup.rs is a rolling
// redirector, so we pin a specific build from the immutable archive instead. This
// only bootstraps rustup itself; `rustup toolchain install` below still fetches the
// latest stable toolchain. To bump: read the current version from
// https://static.rust-lang.org/rustup/release-stable.toml and the matching
// .../x86_64-pc-windows-msvc/rustup-init.exe.sha256.
const RUSTUP_URL: &str =
    "https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-pc-windows-msvc/rustup-init.exe";
const RUSTUP_SHA256: &str = "86478e53f769379d7f0ebfa7c9aa97cb76ca92233f79aa2cc0dbee2efaac73c7";

const MINGIT_URL: &str =
    "https://github.com/git-for-windows/git/releases/download/v2.47.1.windows.1/MinGit-2.47.1-64-bit.zip";
const MINGIT_SHA256: &str = "50b04b55425b5c465d076cdb184f63a0cd0f86f6ec8bb4d5860114a713d2c29a";

// winlibs deletes superseded packaging revisions, and the old `-ucrt-r3` asset is now
// a 404. `-ucrt-r2` is the same compiler (GCC 14.2.0 / LLVM 19.1.1 / mingw-w64 ucrt
// 12.0.0); only the packaging revision differs.
const MINGW_URL: &str = "https://github.com/brechtsanders/winlibs_mingw/releases/download/14.2.0posix-19.1.1-12.0.0-ucrt-r2/winlibs-x86_64-posix-seh-gcc-14.2.0-llvm-19.1.1-mingw-w64ucrt-12.0.0-r2.zip";
const MINGW_SHA256: &str = "12fa72d2566e641c3bf0213a946d33d8bef2e0757af2fb3ed60a995e05d74606";

// pnpm installed before the repo exists. Only a bootstrap — once package.json is on disk
// `ensure_pnpm` pins the exact version the repo asks for, so this drifting out of date
// costs one extra install and nothing else.
const PNPM_BOOTSTRAP_VER: &str = "11.7.0";

const NODE_VER: &str = "v20.18.1";
const NODE_SHA256: &str = "56e5aacdeee7168871721b75819ccacf2367de8761b78eaceacdecd41e04ca03";

// WebView2 evergreen bootstrapper: rolling URL, but Authenticode-signed by Microsoft.
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const WEBVIEW2_SIGNER: &str = "Microsoft Corporation";

const APP_NAME: &str = "xConsole";
const EXE_NAME: &str = "xconsole.exe";
const VERSION: &str = "0.1.0";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\xConsole";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

// Robustness knobs for the long, network-heavy steps. Every external command the
// installer runs is idempotent (downloads truncate+rewrite, extraction overwrites,
// the build resumes incrementally, git re-clones), so a hung or transiently-failing
// attempt is always safe to kill and retry from scratch.
const MAX_ATTEMPTS: u32 = 3;
// Abort a child that produces NO output for this long. Downloads stream a progress
// line every second, extraction prints every few hundred files, and the compiler
// prints continuously — so real work never goes silent this long. Only a child
// blocked on an interactive prompt (the corepack "continue? [Y/n]" hang) does.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
// The final GNU-ld link of the release binary can be silent for a minute or two on a
// slow disk, so the build step gets a more generous silence budget than everything
// else. (A false-kill here would self-heal anyway — the incremental rebuild on retry
// resumes straight to the link — but better not to throw away progress in the first
// place.)
const BUILD_IDLE_TIMEOUT: Duration = Duration::from_secs(900);

const STEPS: [&str; 11] = [
    "Preparing",
    "Setting up Git",
    "Setting up Rust (GNU)",
    "Setting up the C compiler (MinGW)",
    "Setting up Node.js + pnpm",
    "Checking WebView2 runtime",
    "Downloading xConsole source",
    "Installing dependencies",
    "Building xConsole (this can take a while)",
    "Installing app + shortcuts",
    "Finishing up",
];

fn base_dir() -> PathBuf {
    // Optional explicit install root (lets a user/CI choose where xConsole lives;
    // also used for testing outside the default per-user location).
    if let Ok(b) = std::env::var("XCONSOLE_INSTALL_BASE") {
        let b = b.trim();
        // The path is later interpolated into PowerShell download/extract commands, so reject
        // any value carrying shell metacharacters — otherwise a quote could break out of the
        // PS string literal and inject commands. A normal Windows path has none of these.
        let unsafe_char = b.chars().any(|c| "'\"`;&|$<>\r\n".contains(c));
        if !b.is_empty() && !unsafe_char {
            return PathBuf::from(b);
        }
    }
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join(APP_NAME)
}
fn app_dir() -> PathBuf {
    base_dir().join("app")
}
fn src_dir() -> PathBuf {
    base_dir().join("src")
}

// ---- progress reporting -----------------------------------------------------

#[derive(Clone)]
struct Reporter {
    app: Option<AppHandle>,
    log_path: Arc<PathBuf>,
    /// The last few lines logged, so a failing step can say *why* it failed.
    ///
    /// A child's real error — PowerShell's exception, git's message — arrives here as
    /// output and then the step reports a bare "exit 1", which is useless to anyone
    /// without the log file in front of them. This keeps the tail so the message the user
    /// actually sees carries the reason.
    tail: Arc<Mutex<std::collections::VecDeque<String>>>,
}

impl Reporter {
    fn new(app: Option<AppHandle>) -> Self {
        let base = base_dir();
        let _ = std::fs::create_dir_all(&base);
        let log_path = base.join("install.log");
        let _ = std::fs::write(&log_path, "xConsole install log\n");
        Reporter {
            app,
            log_path: Arc::new(log_path),
            tail: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        }
    }

    /// Forget the tail — called before each attempt so the reason belongs to *this* run.
    fn clear_tail(&self) {
        if let Ok(mut t) = self.tail.lock() {
            t.clear();
        }
    }

    /// The last few meaningful output lines, one line, for an error message.
    fn tail(&self) -> String {
        let Ok(t) = self.tail.lock() else {
            return String::new();
        };
        let lines: Vec<&str> = t
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.starts_with("[step "))
            .collect();
        // The last line is usually the exception; the couple before it give it context.
        let take = lines.len().saturating_sub(3);
        lines[take..].join(" / ")
    }
    fn log(&self, line: impl Into<String>) {
        // Everything logged here can come from a child process — vite, cargo, git — and
        // those colour their output when they think a terminal is attached. The installer
        // window is HTML and the log file is plain text, so the escapes render as literal
        // "[32m" noise with the ESC shown as a box glyph. Stripped once, here, because
        // this is the single point every line passes through.
        let line = strip_ansi(&line.into());
        if let Ok(mut t) = self.tail.lock() {
            t.push_back(line.clone());
            while t.len() > 12 {
                t.pop_front();
            }
        }
        if let Some(a) = &self.app {
            let _ = a.emit("install://log", line.clone());
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&*self.log_path) {
            let _ = writeln!(f, "{line}");
        }
    }
    fn step(&self, index: usize, status: &str, ms: Option<u128>) {
        if let Some(a) = &self.app {
            let _ = a.emit(
                "install://step",
                serde_json::json!({ "index": index, "status": status, "ms": ms }),
            );
        }
        self.log(format!(
            "[step {index} {status}{}]",
            ms.map(|m| format!(" {m}ms")).unwrap_or_default()
        ));
    }
    fn plan(&self, steps: &[&str]) {
        if let Some(a) = &self.app {
            let _ = a.emit("install://plan", steps);
        }
        self.log("PLAN:");
        for (i, s) in steps.iter().enumerate() {
            self.log(format!("  {i}. {s}"));
        }
    }
}

// ---- toolchain environment --------------------------------------------------

struct BuildEnv {
    path: String,
    rustup_home: Option<PathBuf>,
    cargo_home: Option<PathBuf>,
}

fn current_env(base: &Path) -> BuildEnv {
    let mut parts: Vec<String> = Vec::new();
    for c in [
        base.join(r"tools\git\cmd"),
        base.join(r"tools\mingw\mingw64\bin"),
        base.join(r"tools\node"),
        base.join(r".cargo\bin"),
    ] {
        if c.exists() {
            parts.push(c.display().to_string());
        }
    }
    if let Ok(p) = std::env::var("PATH") {
        parts.push(p);
    }
    BuildEnv {
        path: parts.join(";"),
        rustup_home: Some(base.join(".rustup")).filter(|p| p.exists()),
        cargo_home: Some(base.join(".cargo")).filter(|p| p.exists()),
    }
}

impl BuildEnv {
    fn apply(&self, cmd: &mut Command) {
        cmd.env("PATH", &self.path);
        if let Some(r) = &self.rustup_home {
            cmd.env("RUSTUP_HOME", r);
        }
        if let Some(c) = &self.cargo_home {
            cmd.env("CARGO_HOME", c);
        }
        // The installer runs headless (no console), so a child that waits for keyboard
        // input would hang forever. corepack is the culprit behind the "stuck on
        // Installing dependencies" bug: the first time it fetches the pinned pnpm it
        // prints "Corepack is about to download ... continue? [Y/n]" and blocks on
        // stdin. Disabling its prompt makes it download unattended; the others pre-empt
        // git-credential / npm-confirmation prompts the same way. (We also feed every
        // child a null stdin as a belt-and-suspenders guard — see stream_once / check.)
        cmd.env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0");
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        // Make cargo's network fetches resilient during the long build on flaky links.
        cmd.env("CARGO_NET_RETRY", "5");
        cmd.env("CARGO_NET_GIT_FETCH_WITH_CLI", "true");
    }
    fn check(&self, command_line: &str) -> bool {
        let mut c = Command::new("cmd");
        c.args(["/C", command_line]);
        self.apply(&mut c);
        c.creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    fn capture(&self, command_line: &str) -> String {
        let mut c = Command::new("cmd");
        c.args(["/C", command_line]);
        self.apply(&mut c);
        c.creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null());
        match c.output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string(),
            Err(_) => String::new(),
        }
    }
}

/// Kill a process AND its whole child tree. The installer launches most tools through
/// `cmd /C`, which then spawns node/git/pnpm/etc. `Child::kill()` only reaps the `cmd`
/// wrapper and leaves the grandchild alive holding our stdout/stderr pipes open — so
/// the reader threads (and the whole install) would block forever. `taskkill /T` takes
/// down the entire tree, which closes the pipes and lets the readers finish.
fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
}

/// Drain one output stream to the reporter, stamping `last` on EVERY chunk of bytes
/// read (not only on newline-terminated lines) so the idle-timeout watchdog never
/// false-kills a phase that reports progress with carriage returns (`git clone
/// --progress`, rustup component download) instead of newlines. Lines are flushed to
/// the log on `\n` OR `\r`, so those CR-updated progress bars also surface to the UI.
fn spawn_reader<R: Read + Send + 'static>(
    stream: Option<R>,
    rep: Reporter,
    last: Arc<Mutex<Instant>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Some(s) = stream {
            let mut reader = BufReader::new(s);
            let mut buf = [0u8; 4096];
            let mut line: Vec<u8> = Vec::new();
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break; // EOF: all writers (incl. inherited grandchild handles) closed
                }
                if let Ok(mut t) = last.lock() {
                    *t = Instant::now();
                }
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        if !line.is_empty() {
                            rep.log(String::from_utf8_lossy(&line).into_owned());
                            line.clear();
                        }
                    } else {
                        line.push(b);
                    }
                }
            }
            if !line.is_empty() {
                rep.log(String::from_utf8_lossy(&line).into_owned());
            }
        }
    })
}

/// Wait for the reader threads to drain, but NEVER block the installer forever doing
/// so. The piped stdout/stderr handles are inherited by the whole descendant tree, so a
/// grandchild that outlives `cmd` (e.g. the esbuild/vite service spawned during the
/// build, or a stray node daemon) keeps the pipe open and the readers never reach EOF.
/// We give them a short grace period; if they're still stuck we kill the tree to force
/// the handles closed; and if a re-parented orphan that `taskkill /T` can't reach STILL
/// wedges a reader, we detach (drop the handle) rather than join — a leaked reader
/// thread is harmless and dies with the installer. This is what stops the join itself
/// from becoming the new hang.
fn finish_readers(pid: u32, t1: std::thread::JoinHandle<()>, t2: std::thread::JoinHandle<()>) {
    let done = |a: &std::thread::JoinHandle<()>, b: &std::thread::JoinHandle<()>| {
        a.is_finished() && b.is_finished()
    };
    let wait_until = |deadline: Instant, a: &std::thread::JoinHandle<()>, b: &std::thread::JoinHandle<()>| {
        while Instant::now() < deadline && !done(a, b) {
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    // 1) Normal case: the child and its tree have exited, EOF arrives within moments.
    wait_until(Instant::now() + Duration::from_secs(5), &t1, &t2);
    // 2) Still draining → a descendant is holding the inherited pipe open. Kill the tree.
    if !done(&t1, &t2) {
        kill_tree(pid);
        wait_until(Instant::now() + Duration::from_secs(5), &t1, &t2);
    }
    // 3) Join if they finished; otherwise leave them detached so we can't hang here.
    if t1.is_finished() {
        let _ = t1.join();
    }
    if t2.is_finished() {
        let _ = t2.join();
    }
}

/// How a single attempt ended — kept distinct so retries can treat a hang (worth
/// retrying) differently from a clean non-zero exit (often a deterministic error).
enum StepErr {
    Timeout(String),
    Failed(String),
}

/// Run a command ONCE, streaming its output, and abort it if it produces no output for
/// `idle`. A stuck child (e.g. one blocked on a prompt) goes silent; a healthy
/// download/extract/build keeps printing — so this catches hangs without ever killing
/// genuine work.
fn stream_once(rep: &Reporter, mut cmd: Command, what: &str, idle: Duration) -> Result<(), StepErr> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Ask the child not to colour its output at all. `strip_ansi` in the reporter is
        // the safety net; this is the fix, and it also keeps the escapes out of anything
        // that measures line length or matches on the text. Each variable covers a
        // different family: NO_COLOR is the cross-tool convention, FORCE_COLOR=0 is what
        // Node tooling (vite, pnpm) reads, and cargo has its own.
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("CARGO_TERM_COLOR", "never")
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd
        .spawn()
        .map_err(|e| StepErr::Failed(format!("{what}: failed to start ({e})")))?;
    let pid = child.id();
    let last = Arc::new(Mutex::new(Instant::now()));
    let t1 = spawn_reader(child.stdout.take(), rep.clone(), last.clone());
    let t2 = spawn_reader(child.stderr.take(), rep.clone(), last.clone());

    let outcome = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(StepErr::Failed(format!("{what}: {e}"))),
        }
        let idle_for = last.lock().map(|t| t.elapsed()).unwrap_or_default();
        if idle_for >= idle {
            rep.log(format!(
                "{what}: no output for {}s — assuming it is stuck, terminating.",
                idle.as_secs()
            ));
            kill_tree(pid);
            let _ = child.wait();
            break Err(StepErr::Timeout(format!(
                "{what} timed out (no output for {}s)",
                idle.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    // Always reap the reader threads through the bounded drainer so a lingering
    // grandchild pipe can never hang the installer (success OR failure path).
    finish_readers(pid, t1, t2);
    match outcome {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => {
            // Report the child's own last words, not just its exit code. "exit 1" tells
            // nobody anything; "Unable to connect to the remote server" tells them
            // everything, and it is already right there in the output we just streamed.
            let why = rep.tail();
            Err(StepErr::Failed(if why.is_empty() {
                format!("{what} failed (exit {})", s.code().unwrap_or(-1))
            } else {
                format!("{what} failed (exit {}): {why}", s.code().unwrap_or(-1))
            }))
        }
        Err(e) => Err(e),
    }
}

/// Run a command with retries and a per-attempt idle timeout. `make` rebuilds a fresh
/// `Command` each attempt (a spawned `Command` can't be reused). A hang (idle-timeout)
/// is ALWAYS retried; a clean non-zero exit is retried only when `retry_on_exit` is set
/// — callers whose failure is usually deterministic (the compile/link in `tauri build`,
/// a `--frozen-lockfile` mismatch) pass `false` so a real error fails fast instead of
/// re-running a 15-minute build three times.
fn run_with_retry(
    rep: &Reporter,
    what: &str,
    attempts: u32,
    idle: Duration,
    retry_on_exit: bool,
    make: impl Fn() -> Command,
) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 1..=attempts {
        rep.clear_tail();
        match stream_once(rep, make(), what, idle) {
            Ok(()) => return Ok(()),
            Err(StepErr::Timeout(e)) => last = e, // a hang — always worth a retry
            Err(StepErr::Failed(e)) => {
                last = e;
                if !retry_on_exit {
                    return Err(format!("{what} failed — {last}"));
                }
            }
        }
        if attempt < attempts {
            rep.log(format!(
                "{what}: attempt {attempt} of {attempts} failed — {last}. Retrying..."
            ));
            std::thread::sleep(Duration::from_secs(3 * attempt as u64));
        }
    }
    Err(format!("{what} failed after {attempts} attempts — {last}"))
}

/// Find an executable/script on the given PATH (tries .exe/.cmd/.bat).
fn resolve(program: &str, path: &str) -> Option<PathBuf> {
    if Path::new(program).is_file() {
        return Some(PathBuf::from(program));
    }
    for dir in path.split(';').filter(|d| !d.is_empty()) {
        // Only Windows-runnable forms — NEVER the extensionless Unix shim (a bash
        // script, which CreateProcess rejects with "not a valid Win32 application").
        for ext in [".exe", ".cmd", ".bat"] {
            let p = Path::new(dir).join(format!("{program}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Run a tool by name with SEPARATE args, so paths never need manual quoting
/// (the previous `cmd /C "<string>"` approach corrupted paths). `.cmd`/`.bat`
/// shims (pnpm, corepack, npm) go through `cmd /C`; real `.exe`s (git, cargo,
/// gcc, node) launch directly.
fn run_tool(
    rep: &Reporter,
    env: &BuildEnv,
    cwd: Option<&Path>,
    program: &str,
    args: &[&str],
    what: &str,
) -> Result<(), String> {
    // Default policy: 3 attempts, default idle budget, and retry transient non-zero
    // exits (a flaky download/clone is worth re-running).
    run_tool_full(rep, env, cwd, program, args, what, MAX_ATTEMPTS, IDLE_TIMEOUT, true)
}

/// `run_tool` with an explicit retry policy. The build step uses this with a longer idle
/// budget (silent final link) and `retry_on_exit=false` (a compile error is
/// deterministic — fail fast instead of rebuilding 3×); the frozen-lockfile probe uses
/// `attempts=1` so it falls straight through to the normal install.
#[allow(clippy::too_many_arguments)]
fn run_tool_full(
    rep: &Reporter,
    env: &BuildEnv,
    cwd: Option<&Path>,
    program: &str,
    args: &[&str],
    what: &str,
    attempts: u32,
    idle: Duration,
    retry_on_exit: bool,
) -> Result<(), String> {
    let full =
        resolve(program, &env.path).ok_or_else(|| format!("{program}: not found on PATH"))?;
    let is_script = full
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "cmd" || e == "bat"
        })
        .unwrap_or(false);
    run_with_retry(rep, what, attempts, idle, retry_on_exit, || {
        let mut c = if is_script {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&full).args(args);
            c
        } else {
            let mut c = Command::new(&full);
            c.args(args);
            c
        };
        env.apply(&mut c);
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        c
    })
}

/// A tool from `%SystemRoot%\System32`, if this Windows has it.
///
/// Looked up by absolute path rather than through `PATH`, so a shim, an App Execution
/// Alias or a user-installed namesake can't be picked up instead.
fn system32(name: &str) -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let p = PathBuf::from(root).join("System32").join(name);
    p.exists().then_some(p)
}

fn download(rep: &Reporter, url: &str, dest: &Path) -> Result<(), String> {
    // Stream the body in 1 MB chunks and print a progress line every second. This
    // replaces `Invoke-WebRequest -OutFile`, which was silent until completion — so a
    // stalled connection looked identical to a healthy slow download and there was no
    // signal for the idle-timeout watchdog. The explicit connect/read timeouts also
    // make a dead socket fail fast (instead of hanging) so a retry can kick in.
    //
    // URL and destination are passed via env vars, never interpolated into the script,
    // so an unusual path can't break out of the PowerShell string literal. Each attempt
    // re-creates (truncates) the file, so a retry after a partial download starts clean.
    const SCRIPT: &str = "\
$ErrorActionPreference='Stop'; \
[Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12; \
$req=[Net.HttpWebRequest]::Create($env:XCV_URL); \
$req.Timeout=30000; $req.ReadWriteTimeout=60000; \
$req.UserAgent='xConsole-Installer'; $req.Accept='*/*'; \
if($req.Proxy){ $req.Proxy.Credentials=[Net.CredentialCache]::DefaultCredentials }; \
$resp=$req.GetResponse(); $total=$resp.ContentLength; \
$in=$resp.GetResponseStream(); $out=[IO.File]::Create($env:XCV_DEST); \
try { \
  $buf=New-Object byte[] 1048576; $read=0; $sw=[Diagnostics.Stopwatch]::StartNew(); \
  while(($n=$in.Read($buf,0,$buf.Length)) -gt 0){ \
    $out.Write($buf,0,$n); $read+=$n; \
    if($sw.Elapsed.TotalSeconds -ge 1){ \
      $t=if($total -gt 0){[string][math]::Round($total/1MB,1)}else{'?'}; \
      Write-Output ('  ' + [math]::Round($read/1MB,1) + ' / ' + $t + ' MB'); \
      $sw.Restart() } } \
  Write-Output ('  done (' + [math]::Round($read/1MB,1) + ' MB)') \
} finally { $out.Close(); $in.Close() }";
    let ps = run_with_retry(rep, "download", MAX_ATTEMPTS, IDLE_TIMEOUT, true, || {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .env("XCV_URL", url)
            .env("XCV_DEST", dest);
        c
    });
    let Err(e) = ps else { return Ok(()) };

    // Fall back to the curl that ships in System32 (Windows 10 1803 and later).
    //
    // This whole script is one long `-Command` string, which is exactly what a machine
    // running PowerShell in Constrained Language Mode, or with an execution policy set by
    // group policy, will refuse — common on managed Windows 10 boxes and invisible from
    // here except as a failing first step. curl also handles proxies and TLS through
    // WinHTTP rather than .NET, so it survives a different set of network setups.
    let Some(curl) = system32("curl.exe") else {
        return Err(e);
    };
    rep.log(format!("PowerShell could not download it ({e})."));
    rep.log("Retrying with curl...");
    run_with_retry(rep, "download (curl)", MAX_ATTEMPTS, IDLE_TIMEOUT, true, || {
        let mut c = Command::new(&curl);
        c.args([
            "-fL", // fail on an HTTP error instead of saving the error page
            "--connect-timeout",
            "30",
            // Give up on a connection that stalls below 1 byte/s for a minute, so a dead
            // socket fails fast enough for the retry to be worth something.
            "--speed-limit",
            "1",
            "--speed-time",
            "60",
            "-A",
            "xConsole-Installer",
            "--output",
        ])
        .arg(dest)
        .arg(url);
        c
    })
}

fn unzip(rep: &Reporter, zip: &Path, dest: &Path) -> Result<(), String> {
    // Extract entry-by-entry, printing progress every 200 files. `Expand-Archive` was
    // silent for the whole extraction — and the MinGW archive alone is ~1.5 GB
    // unpacked, several silent minutes that would trip the idle-timeout watchdog. This
    // also overwrites in place (so a retry can resume) and rejects any entry whose path
    // escapes the destination (zip-slip), even though every archive here is SHA-pinned.
    //
    // Runs after verify_sha256, so the archive is already trusted; paths come via env
    // vars so they're never interpolated into the script.
    const SCRIPT: &str = "\
$ErrorActionPreference='Stop'; \
Add-Type -AssemblyName System.IO.Compression.FileSystem; \
$zip=[IO.Compression.ZipFile]::OpenRead($env:XCV_ZIP); \
try { \
  $root=[IO.Path]::GetFullPath($env:XCV_DEST); \
  if(-not $root.EndsWith([IO.Path]::DirectorySeparatorChar)){ $root=$root + [IO.Path]::DirectorySeparatorChar } \
  $i=0; $tot=$zip.Entries.Count; \
  foreach($e in $zip.Entries){ \
    $i++; \
    $target=[IO.Path]::GetFullPath([IO.Path]::Combine($root, $e.FullName)); \
    if(-not $target.StartsWith($root)){ throw ('unsafe zip entry: ' + $e.FullName) }; \
    if($e.FullName.EndsWith('/')){ continue }; \
    $dir=[IO.Path]::GetDirectoryName($target); \
    if($dir -and -not (Test-Path -LiteralPath $dir)){ New-Item -ItemType Directory -LiteralPath $dir -Force | Out-Null }; \
    [IO.Compression.ZipFileExtensions]::ExtractToFile($e, $target, $true); \
    if($i % 200 -eq 0){ Write-Output ('  extracted ' + $i + ' / ' + $tot + ' files') } } \
  Write-Output ('  done (' + $tot + ' files)') \
} finally { $zip.Dispose() }";
    let ps = run_with_retry(rep, "extract", MAX_ATTEMPTS, IDLE_TIMEOUT, true, || {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .env("XCV_ZIP", zip)
            .env("XCV_DEST", dest);
        c
    });
    let Err(e) = ps else { return Ok(()) };

    // Same fallback reasoning as `download`: bsdtar in System32 reads zip files and needs
    // no PowerShell at all. It also copes with paths past MAX_PATH, which the .NET
    // extractor above does not — MinGit's deepest entries plus a long user name can cross
    // 260 characters on a machine without long paths enabled.
    let Some(tar) = system32("tar.exe") else {
        return Err(e);
    };
    rep.log(format!("PowerShell could not extract it ({e})."));
    rep.log("Retrying with tar...");
    std::fs::create_dir_all(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    // tar is quiet, and listing every one of MinGW's ~30k files would flood the log to no
    // purpose — so it gets the long silence budget instead of the usual one.
    run_with_retry(rep, "extract (tar)", MAX_ATTEMPTS, BUILD_IDLE_TIMEOUT, true, || {
        let mut c = Command::new(&tar);
        c.arg("-xf").arg(zip).arg("-C").arg(dest);
        c
    })
}

/// Stream a file through SHA-256 and return the lowercase hex digest.
fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Verify a downloaded artifact against its pinned SHA-256 BEFORE it is unzipped or
/// run. On mismatch the file is deleted (so a re-run re-downloads it) and the step
/// fails hard — we never extract or execute an artifact that failed its check.
fn verify_sha256(rep: &Reporter, path: &Path, expected: &str, what: &str) -> Result<(), String> {
    let actual = sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        rep.log(format!("Verified {what} (SHA-256 {actual})."));
        Ok(())
    } else {
        let _ = std::fs::remove_file(path);
        Err(format!(
            "{what} failed its integrity check (expected SHA-256 {expected}, got {actual}). \
             The download was corrupt or tampered with — aborting before it could be used."
        ))
    }
}

/// Verify an Authenticode signature for artifacts that roll (no stable hash) but are
/// code-signed: require Status=Valid AND a signer subject naming the expected org.
/// The file path and expected signer are passed via env vars (never interpolated into
/// the script) so an unusual path can't break out of the PowerShell string literal.
fn verify_authenticode(
    rep: &Reporter,
    path: &Path,
    expected_signer: &str,
    what: &str,
) -> Result<(), String> {
    // `Stop` makes a failing Get-AuthenticodeSignature throw (-> non-zero exit). The
    // validation failures use [Console]::Error.WriteLine (not Write-Error) so the exit
    // codes actually fire and stderr carries a clean one-line reason, not PS's error blob.
    const SCRIPT: &str = "\
$ErrorActionPreference = 'Stop'; \
$sig = Get-AuthenticodeSignature -LiteralPath $env:XCV_FILE; \
if ($sig.Status -ne 'Valid') { [Console]::Error.WriteLine('status=' + $sig.Status); exit 2 }; \
$subject = $sig.SignerCertificate.Subject; \
if ($subject -notlike ('*' + $env:XCV_SIGNER + '*')) { [Console]::Error.WriteLine('signer=' + $subject); exit 3 }; \
Write-Output $subject";
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("XCV_FILE", path)
        .env("XCV_SIGNER", expected_signer)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("{what}: could not run the signature check ({e})"))?;
    if out.status.success() {
        let subject = String::from_utf8_lossy(&out.stdout).trim().to_string();
        rep.log(format!("Verified {what} Authenticode signature ({subject})."));
        Ok(())
    } else {
        let _ = std::fs::remove_file(path);
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!(
            "{what} failed Authenticode verification ({err}). \
             Refusing to run an unsigned or untrusted binary."
        ))
    }
}

/// Remove a directory tree, falling back to `rmdir /s /q` for git's read-only files.
fn remove_dir_robust(p: &Path) {
    if !p.exists() {
        return;
    }
    let _ = std::fs::remove_dir_all(p);
    if p.exists() {
        let _ = Command::new("cmd")
            .args(["/C", &format!("rmdir /s /q \"{}\"", p.display())])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}

fn clone_fresh(rep: &Reporter, env: &BuildEnv, src: &Path) -> Result<(), String> {
    remove_dir_robust(src);
    if src.exists() {
        return Err(format!("could not clear existing folder {}", src.display()));
    }
    rep.log(format!("Cloning {REPO_URL} into {}", src.display()));
    let dest = src.display().to_string();
    // schannel = use the Windows certificate store (robust on minimal/portable gits).
    if run_tool(
        rep,
        env,
        None,
        "git",
        &[
            "-c", "http.sslBackend=schannel", "clone", "--depth", "1", "--progress", REPO_URL,
            &dest,
        ],
        "git clone",
    )
    .is_ok()
    {
        return Ok(());
    }
    rep.log("Shallow clone failed — retrying a full clone...");
    remove_dir_robust(src);
    run_tool(
        rep,
        env,
        None,
        "git",
        &["-c", "http.sslBackend=schannel", "clone", "--progress", REPO_URL, &dest],
        "git clone",
    )
}

// ---- the install ------------------------------------------------------------

pub fn run_install_headless() -> i32 {
    let rep = Reporter::new(None);
    match run_install(&rep) {
        Ok(()) => {
            rep.log("INSTALL OK");
            0
        }
        Err(e) => {
            rep.log(format!("INSTALL FAILED: {e}"));
            1
        }
    }
}

/// True when launched as `--update` (by the app's in-app updater) — the frontend uses
/// this to skip the welcome screen and auto-start the rebuild.
#[tauri::command]
pub fn is_update_mode() -> bool {
    std::env::args().any(|a| a == "--update")
}

#[tauri::command]
pub fn start_install(app: AppHandle) {
    std::thread::spawn(move || {
        let rep = Reporter::new(Some(app.clone()));
        match run_install(&rep) {
            Ok(()) => {
                let _ = app.emit(
                    "install://done",
                    serde_json::json!({ "ok": true, "error": serde_json::Value::Null }),
                );
            }
            Err(e) => {
                rep.log(format!("ERROR: {e}"));
                let _ = app.emit("install://done", serde_json::json!({ "ok": false, "error": e }));
            }
        }
    });
}

fn run_install(rep: &Reporter) -> Result<(), String> {
    rep.plan(&STEPS);
    let base = base_dir();

    macro_rules! step {
        ($idx:expr, $body:block) => {{
            let t = Instant::now();
            rep.step($idx, "running", None);
            let r: Result<(), String> = (|| $body)();
            match r {
                Ok(()) => rep.step($idx, "done", Some(t.elapsed().as_millis())),
                Err(e) => {
                    rep.step($idx, "error", None);
                    return Err(e);
                }
            }
        }};
    }

    // 0. Preparing
    step!(0, {
        std::fs::create_dir_all(base.join("tools")).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
        // The previous updater, renamed aside so the new one could take its place. It is
        // no longer running by now, so it can finally go.
        let _ = std::fs::remove_file(base.join("uninstall.old.exe"));
        rep.log("xConsole setup — building from source (Hermes-style).");
        rep.log(format!("Install location: {}", base.display()));
        rep.log("First run downloads the toolchain + compiles; this can take 10-20 minutes.");
        Ok(())
    });

    // 1. Git
    step!(1, {
        let env = current_env(&base);
        if env.check("git --version") {
            rep.log(format!("Git is already available ({}).", env.capture("git --version")));
        } else {
            rep.log("Installing a portable Git...");
            let zip = base.join("tools").join("mingit.zip");
            download(rep, MINGIT_URL, &zip)?;
            verify_sha256(rep, &zip, MINGIT_SHA256, "MinGit")?;
            unzip(rep, &zip, &base.join(r"tools\git"))?;
            let _ = std::fs::remove_file(&zip);
            if !current_env(&base).check("git --version") {
                return Err(diagnose_git(rep, &base));
            }
        }
        Ok(())
    });

    // 2. Rust (GNU toolchain)
    step!(2, {
        let env = current_env(&base);
        if !env.check("rustup --version") {
            rep.log("Installing the Rust toolchain (rustup)...");
            let init = base.join("tools").join("rustup-init.exe");
            download(rep, RUSTUP_URL, &init)?;
            verify_sha256(rep, &init, RUSTUP_SHA256, "rustup-init")?;
            let portable = BuildEnv {
                path: env.path.clone(),
                rustup_home: Some(base.join(".rustup")),
                cargo_home: Some(base.join(".cargo")),
            };
            run_with_retry(rep, "rustup install", MAX_ATTEMPTS, IDLE_TIMEOUT, true, || {
                let mut c = Command::new(&init);
                c.args([
                    "--default-host",
                    "x86_64-pc-windows-gnu",
                    "--default-toolchain",
                    "stable",
                    "--profile",
                    "minimal",
                    "-y",
                ]);
                portable.apply(&mut c);
                c
            })?;
            let _ = std::fs::remove_file(&init);
        } else {
            rep.log("Rust is already available.");
        }
        let env = current_env(&base);
        rep.log("Ensuring the GNU toolchain is installed (needed to link without MSVC)...");
        run_tool(
            rep,
            &env,
            None,
            "rustup",
            &["toolchain", "install", GNU_TOOLCHAIN, "--no-self-update"],
            "rustup toolchain",
        )?;
        Ok(())
    });

    // 3. MinGW
    step!(3, {
        let env = current_env(&base);
        if env.check("gcc --version") {
            rep.log("A C compiler (gcc) is already available.");
        } else {
            rep.log("Installing the MinGW compiler...");
            let zip = base.join("tools").join("mingw.zip");
            download(rep, MINGW_URL, &zip)?;
            verify_sha256(rep, &zip, MINGW_SHA256, "MinGW")?;
            unzip(rep, &zip, &base.join(r"tools\mingw"))?;
            let _ = std::fs::remove_file(&zip);
            if !current_env(&base).check("gcc --version") {
                return Err("MinGW did not install correctly.".into());
            }
        }
        Ok(())
    });

    // 4. Node + pnpm
    step!(4, {
        let env = current_env(&base);
        if !env.check("node --version") {
            rep.log("Installing a portable Node.js...");
            let zip = base.join("tools").join("node.zip");
            let url = format!("https://nodejs.org/dist/{NODE_VER}/node-{NODE_VER}-win-x64.zip");
            download(rep, &url, &zip)?;
            verify_sha256(rep, &zip, NODE_SHA256, "Node.js")?;
            unzip(rep, &zip, &base.join("tools"))?;
            let _ = std::fs::remove_file(&zip);
            let extracted = base.join(format!("tools\\node-{NODE_VER}-win-x64"));
            let target = base.join(r"tools\node");
            remove_dir_robust(&target);
            std::fs::rename(&extracted, &target).map_err(|e| format!("node layout: {e}"))?;
        } else {
            rep.log("Node.js is already available.");
        }
        let env = current_env(&base);
        if env.check("pnpm --version") {
            rep.log(format!(
                "pnpm is already available ({}).",
                env.capture("pnpm --version")
            ));
        } else {
            // npm, not corepack — see `ensure_pnpm` for why corepack cannot be relied on
            // here. This is only a bootstrap; step 7 pins whatever version the repo asks
            // for once its package.json exists.
            rep.log("Installing pnpm...");
            let spec = format!("pnpm@{PNPM_BOOTSTRAP_VER}");
            if run_tool(rep, &env, None, "npm", &["install", "-g", &spec], "npm i -g pnpm").is_err()
            {
                rep.log("npm could not install pnpm — falling back to corepack.");
                run_tool(rep, &env, None, "corepack", &["enable", "pnpm"], "corepack")?;
            }
        }
        Ok(())
    });

    // 5. WebView2 runtime
    step!(5, {
        if webview2_installed() {
            rep.log("WebView2 runtime is already installed.");
        } else {
            rep.log("Installing the Microsoft WebView2 runtime...");
            match install_webview2(rep) {
                Ok(()) => rep.log("WebView2 runtime installed."),
                Err(e) => rep.log(format!("WebView2 warning (non-fatal): {e}")),
            }
        }
        Ok(())
    });

    // 6. Source
    step!(6, {
        let env = current_env(&base);
        let src = src_dir();
        rep.log(format!("Using git: {}", env.capture("where git")));
        if src.join(".git").exists() {
            rep.log("Updating existing source (git fetch + reset)...");
            let ok = run_tool(rep, &env, Some(&src), "git", &["fetch", "--depth", "1", "origin", "main"], "git fetch").is_ok()
                && run_tool(rep, &env, Some(&src), "git", &["reset", "--hard", "origin/main"], "git reset").is_ok();
            if !ok {
                rep.log("Update failed — re-cloning fresh...");
                clone_fresh(rep, &env, &src)?;
            }
        } else {
            clone_fresh(rep, &env, &src)?;
        }
        Ok(())
    });

    // 7. Frontend dependencies
    step!(7, {
        let env = current_env(&base);
        let src = src_dir();
        // The repo is on disk now, so its package.json can say which pnpm it wants. Doing
        // this here rather than in step 4 means the version is never guessed.
        ensure_pnpm(rep, &env, &src);
        let env = current_env(&base);
        rep.log("Installing frontend dependencies (pnpm install)...");
        // --frozen-lockfile installs exactly what pnpm-lock.yaml pins, skipping the
        // resolution pass — faster and bit-for-bit identical to what CI ships.
        // --prefer-offline reuses the global pnpm store so re-installs/updates barely
        // touch the network. The frozen attempt is single-shot (attempts=1): a lockfile
        // mismatch is deterministic, so don't burn the 3× retry+backoff on it — fall
        // straight through to a normal resolving install so the build still succeeds.
        if run_tool_full(
            rep,
            &env,
            Some(&src),
            "pnpm",
            &["install", "--frozen-lockfile", "--prefer-offline"],
            "pnpm install (frozen)",
            1,
            IDLE_TIMEOUT,
            false,
        )
        .is_err()
        {
            rep.log("Frozen install failed (lockfile out of sync?) — falling back to a normal install...");
            run_tool(rep, &env, Some(&src), "pnpm", &["install"], "pnpm install")?;
        }
        Ok(())
    });

    // 8. Build — MUST use `tauri build` (production), NOT raw `cargo build`.
    //    Raw `cargo build` yields a dev-mode app that loads the (nonexistent) dev
    //    server at http://localhost:1420; `tauri build` embeds + serves the frontend.
    step!(8, {
        let env = current_env(&base);
        let src = src_dir();
        // Pin the GNU toolchain for the clone so tauri build's cargo links without MSVC.
        rep.log("Selecting the GNU toolchain for the build...");
        run_tool(
            rep,
            &env,
            Some(&src),
            "rustup",
            &["override", "set", GNU_TOOLCHAIN],
            "rustup override",
        )?;
        // Override beforeBuildCommand to skip the repo's standalone `tsc` typecheck
        // (it breaks on pnpm's symlinked .bin shim here; vite/esbuild strips types
        // anyway and produces the same dist).
        let cfg = src.join(".xconsole-build.json");
        std::fs::write(&cfg, r#"{"build":{"beforeBuildCommand":"pnpm exec vite build"}}"#)
            .map_err(|e| format!("write build config: {e}"))?;
        rep.log("Building xConsole (production, release). This is the long part (~10-20 min)...");
        // retry_on_exit=false: a compile/link failure is deterministic, so fail fast
        // rather than re-run a 15-minute build 3×. A genuine HANG (idle-timeout) is still
        // retried, and cargo's own CARGO_NET_RETRY handles transient crate-fetch flakiness.
        run_tool_full(
            rep,
            &env,
            Some(&src),
            "pnpm",
            &["exec", "tauri", "build", "--no-bundle", "-c", ".xconsole-build.json"],
            "tauri build",
            MAX_ATTEMPTS,
            BUILD_IDLE_TIMEOUT,
            false,
        )?;
        Ok(())
    });

    // 9. Install built app + shortcuts
    step!(9, {
        let built = src_dir().join(r"src-tauri\target\release");
        let exe = built.join(EXE_NAME);
        if !exe.exists() {
            return Err(format!("build did not produce {}", exe.display()));
        }
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", EXE_NAME])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        std::fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
        std::fs::copy(&exe, app_dir().join(EXE_NAME)).map_err(|e| format!("copy exe: {e}"))?;
        rep.log(format!("Installed {EXE_NAME}"));
        let dll = built.join("WebView2Loader.dll");
        if dll.exists() {
            let _ = std::fs::copy(&dll, app_dir().join("WebView2Loader.dll"));
            rep.log("Installed WebView2Loader.dll");
        }
        match create_shortcuts(&app_dir().join(EXE_NAME)) {
            Ok(made) => {
                for m in made {
                    rep.log(format!("Created {m}"));
                }
            }
            Err(e) => rep.log(format!("shortcut warning: {e}")),
        }
        Ok(())
    });

    // 10. Finishing up
    step!(10, {
        // Install `uninstall.exe` — re-used for both "Apps & Features" removal AND the
        // in-app `uninstall.exe --update`. It MUST be runnable on its own. When we were
        // launched from the single-exe self-extracting stub (GNU build), our own
        // current_exe() is the DLL-needing inner exe in %TEMP%; the stub passes its own
        // self-contained path in XC_OUTER_EXE, so register THAT instead. Under MSVC
        // (statically linked, no stub) XC_OUTER_EXE is unset and current_exe() is itself
        // already self-contained.
        let uninstaller_src = std::env::var_os("XC_OUTER_EXE")
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| std::env::current_exe().ok());
        if let Some(me) = uninstaller_src {
            let dest = base.join("uninstall.exe");
            // On the in-app `--update` flow we're launched as base\uninstall.exe (the
            // stub), so XC_OUTER_EXE == dest: copying it onto the live, running file would
            // just fail with a sharing violation. Skip the no-op self-copy; otherwise
            // refresh the registered uninstaller and surface (don't swallow) a real error.
            let same = std::fs::canonicalize(&me)
                .ok()
                .zip(std::fs::canonicalize(&dest).ok())
                .map(|(a, b)| a == b)
                .unwrap_or(false);
            if !same {
                if let Err(e) = std::fs::copy(&me, &dest) {
                    rep.log(format!("note: could not refresh uninstall.exe ({e})"));
                }
                // This exe was built from the source we just cloned, so stamp it as
                // current — otherwise the first update would rebuild it for nothing.
                let rev = current_env(&base).capture(&format!(
                    "git -C \"{}\" rev-parse HEAD:installer",
                    src_dir().display()
                ));
                if rev.len() >= 40 && rev.chars().all(|c| c.is_ascii_hexdigit()) {
                    let _ = std::fs::write(base.join("updater.rev"), &rev);
                }
            } else {
                // Same file: this is the in-app update flow, running as the very binary
                // it would be copying. That is the one case where the updater can go
                // stale forever, so rebuild it from the source we just pulled.
                refresh_updater(rep, &base);
            }
        }
        register_uninstall(&base).map_err(|e| format!("registry: {e}"))?;
        rep.log("Registered in Apps & Features.");
        rep.log("Done — press Launch xConsole when you're ready.");
        Ok(())
    });

    Ok(())
}

/// The version in `package.json`'s `packageManager` field, e.g. `11.7.0`.
///
/// Parsed by hand rather than with serde_json's full DOM because the field is a single
/// flat string and this must not fail the install if the file grows something unexpected.
fn package_manager_pnpm_version(package_json: &Path) -> Option<String> {
    let text = std::fs::read_to_string(package_json).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let spec = v.get("packageManager")?.as_str()?;
    let rest = spec.strip_prefix("pnpm@")?;
    // Drop any `+sha512...` integrity suffix; npm wants the bare version.
    let ver = rest.split('+').next()?.trim();
    (!ver.is_empty()).then(|| ver.to_string())
}

/// Make sure the pnpm on PATH is the version this checkout asks for, installing it with
/// npm if not.
///
/// **This deliberately routes around corepack.** corepack ships *inside* the Node release
/// the installer pins, and it verifies the pnpm tarball against npm registry signing keys
/// that are hardcoded in that copy. Those keys have been rotated since, so the corepack
/// bundled with Node 20.18.1 cannot verify any current pnpm and dies inside
/// `Engine.executePackageManagerRequest` — reported here as `pnpm install failed after 3
/// attempts` with a corepack stack and nothing else. The same copy also predates pnpm 11
/// and cannot resolve it. Both faults come from pinning Node, which we do on purpose, and
/// neither is fixable from this side.
///
/// npm checks the same tarball against the integrity hash the registry serves for it, so
/// the supply-chain guarantee is kept; it just does not carry a key set that expires with
/// the Node pin. Non-fatal: if this cannot work out or install the right version, the
/// install proceeds with whatever pnpm is there, which may well be fine.
fn ensure_pnpm(rep: &Reporter, env: &BuildEnv, src: &Path) {
    let Some(want) = package_manager_pnpm_version(&src.join("package.json")) else {
        return;
    };
    let have = env.capture("pnpm --version");
    if have == want {
        rep.log(format!("pnpm {have} matches the version this build expects."));
        return;
    }
    rep.log(format!(
        "This build expects pnpm {want}{} — installing it.",
        if have.is_empty() {
            String::new()
        } else {
            format!(" (found {have})")
        }
    ));
    // Remove corepack's shims first. A previous run of this installer enabled them, and
    // they occupy the exact filenames npm is about to write — npm refuses to clobber a
    // shim it did not create, so without this the install fails with EEXIST on a machine
    // that has already tried once. Best effort: absent shims make this a no-op.
    env.check("corepack disable pnpm");
    let spec = format!("pnpm@{want}");
    if let Err(e) = run_tool(rep, env, None, "npm", &["install", "-g", &spec], "npm i -g pnpm") {
        rep.log(format!("note: could not install pnpm {want} ({e}) — continuing with what is here."));
    }
}

/// Work out *why* the freshly-extracted Git will not run, and say so.
///
/// "Git did not install correctly" is true and useless: the download succeeded, the
/// archive passed its SHA-256, the extraction reported done, and then nothing works. The
/// three real causes look completely different on disk, and only one of them is something
/// the user can act on — so name which one it is.
fn diagnose_git(rep: &Reporter, base: &Path) -> String {
    let git_dir = base.join(r"tools\git");
    let exe = git_dir.join(r"cmd\git.exe");

    if !git_dir.exists() {
        return format!(
            "Git did not install: nothing was extracted to {}. \
             Check that the disk is not full and that the folder is writable.",
            git_dir.display()
        );
    }
    if !exe.exists() {
        // The archive was verified before extraction, so a missing git.exe means it was
        // removed *after* it was written — which on Windows means antivirus took it.
        let count = std::fs::read_dir(&git_dir).map(|d| d.count()).unwrap_or(0);
        return format!(
            "Git was extracted ({count} entries under {}) but {} is missing. \
             The archive passed its SHA-256 check before extraction, so something removed \
             the file afterwards — almost always antivirus quarantining an unsigned \
             executable. Add an exclusion for {} and run setup again.",
            git_dir.display(),
            exe.display(),
            base.display()
        );
    }

    // It is on disk. Run it directly, bypassing PATH, and report what Windows says —
    // "Access is denied" (blocked by policy or AV), a missing DLL, and a broken PATH are
    // all different problems with the same symptom until you look.
    let direct = Command::new(&exe)
        .arg("--version")
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output();
    match direct {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            rep.log(format!("{} runs directly ({v}) but not via PATH.", exe.display()));
            format!(
                "Git installed and runs ({v}), but not through PATH. \
                 This usually means the PATH the installer builds is too long for this \
                 machine. Installing Git for Windows normally and re-running setup will \
                 skip this step entirely."
            )
        }
        Ok(o) => format!(
            "Git installed but will not run: {} exited with {}. {}",
            exe.display(),
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!(
            "Git installed but could not be started: {} ({e}). \
             On Windows this is normally antivirus or a policy blocking an unsigned \
             executable — add an exclusion for {} and run setup again.",
            exe.display(),
            base.display()
        ),
    }
}

/// Rebuild `uninstall.exe` from the source we just pulled, when the installer's own
/// source changed.
///
/// `uninstall.exe` is a copy of the setup exe taken at install time, and it is what the
/// in-app updater re-runs. Nothing ever replaced it, so a fix to the installer could
/// never reach anyone who had already installed: the update rebuilt the *app* from new
/// source and then handed control back to the same old updater binary. Colour codes
/// showing up as `[32m` in this very window was the symptom that made it obvious.
///
/// Guarded by the git tree hash of `installer/`, so the usual update — which does not
/// touch the installer — pays nothing. Non-fatal throughout: a failure here leaves the
/// working updater in place, and the app is already installed by this point.
fn refresh_updater(rep: &Reporter, base: &Path) {
    let env = current_env(base);
    let src = src_dir();
    let installer_src = src.join("installer");
    if !installer_src.exists() {
        return;
    }
    // The tree hash changes only when something under installer/ changes.
    let rev = env.capture(&format!(
        "git -C \"{}\" rev-parse HEAD:installer",
        src.display()
    ));
    if rev.len() < 40 || !rev.chars().all(|c| c.is_ascii_hexdigit()) {
        return; // can't tell — don't spend 10 minutes guessing
    }
    let stamp = base.join("updater.rev");
    if std::fs::read_to_string(&stamp).map(|s| s.trim() == rev).unwrap_or(false) {
        return;
    }

    rep.log("The updater itself changed — rebuilding it so the next update runs the new one.");
    if let Err(e) = run_tool_full(
        rep,
        &env,
        Some(&installer_src),
        "cargo",
        &["build", "--release"],
        "updater build",
        1,
        BUILD_IDLE_TIMEOUT,
        false,
    ) {
        rep.log(format!("note: could not rebuild the updater ({e}) — keeping the current one."));
        return;
    }

    let built = installer_src.join(r"target\release\xConsole-Setup.exe");
    if !built.exists() {
        rep.log("note: the updater build produced no exe — keeping the current one.");
        return;
    }

    // We are running *as* base\uninstall.exe, so it cannot be overwritten — but Windows
    // does allow a running image to be renamed. Move ourselves aside, drop the new binary
    // in, and clean the leftover up on the next run. If the copy fails, move back: a
    // missing updater would strand the install with no way to update or uninstall.
    let dest = base.join("uninstall.exe");
    let old = base.join("uninstall.old.exe");
    let _ = std::fs::remove_file(&old);
    let moved = dest.exists();
    if moved {
        if let Err(e) = std::fs::rename(&dest, &old) {
            rep.log(format!("note: could not replace the updater ({e}) — keeping the current one."));
            return;
        }
    }
    if let Err(e) = std::fs::copy(&built, &dest) {
        if moved {
            let _ = std::fs::rename(&old, &dest);
        }
        rep.log(format!("note: could not install the new updater ({e}) — kept the current one."));
        return;
    }
    // Under the GNU toolchain the WebView2 loader is linked dynamically, so it has to sit
    // beside the exe. (The released setup exe embeds it in a self-extracting stub; here we
    // own the folder, so a sibling file is simpler and does the same job.)
    let dll = installer_src.join(r"target\release\WebView2Loader.dll");
    if dll.exists() {
        let _ = std::fs::copy(&dll, base.join("WebView2Loader.dll"));
    }
    let _ = std::fs::write(&stamp, &rev);
    rep.log("Updater refreshed.");
}

// ---- WebView2 / shortcuts / registry / uninstall ---------------------------

fn webview2_installed() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    const CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let checks = [
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\{CLIENT}")),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\{CLIENT}")),
        (HKEY_CURRENT_USER, format!(r"SOFTWARE\{CLIENT}")),
    ];
    for (root, path) in checks {
        if let Ok(k) = RegKey::predef(root).open_subkey(&path) {
            if let Ok(pv) = k.get_value::<String, _>("pv") {
                if !pv.is_empty() && pv != "0.0.0.0" {
                    return true;
                }
            }
        }
    }
    false
}

fn install_webview2(rep: &Reporter) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
    download(rep, WEBVIEW2_URL, &tmp)?;
    // Rolling URL (no stable hash), but Microsoft Authenticode-signs it — verify the
    // signature before we execute the bootstrapper.
    verify_authenticode(rep, &tmp, WEBVIEW2_SIGNER, "WebView2 bootstrapper")?;
    // This silent GUI installer produces no stdout to monitor, so it gets a bounded
    // WALL-CLOCK timeout (not the idle watchdog): null stdin so it can't block on a
    // prompt, and kill the tree if it stalls (e.g. a proxied evergreen fetch) so a hung
    // bootstrapper can't freeze the whole install. The step is non-fatal either way.
    let mut child = Command::new(&tmp)
        .args(["/silent", "/install"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("run bootstrapper: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let st = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => return Err(format!("run bootstrapper: {e}")),
        }
        if Instant::now() >= deadline {
            kill_tree(child.id());
            return Err("WebView2 bootstrapper timed out".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    if !st.success() {
        return Err("WebView2 bootstrapper returned an error".into());
    }
    Ok(())
}

fn create_shortcuts(target: &Path) -> Result<Vec<String>, String> {
    use mslnk::ShellLink;
    let mut made = Vec::new();
    let link = ShellLink::new(target).map_err(|e| e.to_string())?;
    if let Ok(appdata) = std::env::var("APPDATA") {
        let sm = PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{APP_NAME}.lnk"));
        if link.create_lnk(&sm).is_ok() {
            made.push("Start Menu shortcut".to_string());
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let desk = PathBuf::from(profile)
            .join("Desktop")
            .join(format!("{APP_NAME}.lnk"));
        if link.create_lnk(&desk).is_ok() {
            made.push("Desktop shortcut".to_string());
        }
    }
    Ok(made)
}

fn register_uninstall(base: &Path) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(UNINSTALL_KEY)
        .map_err(|e| e.to_string())?;
    let exe = app_dir().join(EXE_NAME);
    let uninstaller = base.join("uninstall.exe");
    key.set_value("DisplayName", &APP_NAME.to_string())
        .map_err(|e| e.to_string())?;
    let _ = key.set_value("DisplayVersion", &VERSION.to_string());
    let _ = key.set_value("Publisher", &"xConsole".to_string());
    let _ = key.set_value("DisplayIcon", &exe.display().to_string());
    let _ = key.set_value(
        "UninstallString",
        &format!("\"{}\" --uninstall", uninstaller.display()),
    );
    let _ = key.set_value("InstallLocation", &base.display().to_string());
    let _ = key.set_value("NoModify", &1u32);
    let _ = key.set_value("NoRepair", &1u32);
    Ok(())
}

#[tauri::command]
pub fn launch_app(app: AppHandle) {
    let exe = app_dir().join(EXE_NAME);
    let _ = Command::new(&exe).spawn();
    app.exit(0);
}

#[tauri::command]
pub fn close_installer(app: AppHandle) {
    app.exit(0);
}

pub fn run_uninstall() {
    use winreg::enums::*;
    use winreg::RegKey;
    let base = base_dir();
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", EXE_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(appdata) = std::env::var("APPDATA") {
        let _ = std::fs::remove_file(
            PathBuf::from(appdata)
                .join(r"Microsoft\Windows\Start Menu\Programs")
                .join(format!("{APP_NAME}.lnk")),
        );
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let _ = std::fs::remove_file(
            PathBuf::from(profile)
                .join("Desktop")
                .join(format!("{APP_NAME}.lnk")),
        );
    }
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(UNINSTALL_KEY);
    // `base\uninstall.exe` is THIS still-running process's image, so it can't be deleted
    // until we exit and Windows releases the handle. A single fixed wait can lose that
    // race on a loaded machine (leaving an orphaned dir), so try a few times with
    // escalating waits — a plain `&` chain (like the original, just repeated) so the
    // cmd string stays trivially parseable. rmdir on an already-gone dir is a harmless
    // no-op. Detached so it outlives us.
    let b = base.display().to_string();
    let _ = Command::new("cmd")
        .args([
            "/C",
            &format!(
                "timeout /t 2 /nobreak >nul & rmdir /s /q \"{b}\" 2>nul & \
                 timeout /t 2 /nobreak >nul & rmdir /s /q \"{b}\" 2>nul & \
                 timeout /t 3 /nobreak >nul & rmdir /s /q \"{b}\" 2>nul"
            ),
        ])
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn();
}

/// Remove ANSI escape sequences from a line of child-process output.
///
/// Handles the two forms that actually turn up here: CSI (`ESC [ … final`) for colour and
/// cursor moves, which is what vite and cargo emit, and OSC (`ESC ] … BEL` or `ESC \`),
/// which some tools use to set the window title. A bare `ESC` with nothing recognisable
/// after it is dropped on its own rather than swallowing the rest of the line — a
/// truncated escape must not eat the message it was attached to.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            // Carriage returns come from progress bars redrawing a line; keeping them
            // makes the log file look empty in editors that honour them.
            if c != '\r' {
                out.push(c);
            }
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // Parameter and intermediate bytes, then one final byte in @..~.
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // Runs to BEL, or to the ST pair ESC \.
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-character escape (e.g. ESC c): drop both.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod pnpm_version_tests {
    use super::package_manager_pnpm_version;

    fn parse(json: &str) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("xc-pm-{}", json.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("package.json");
        std::fs::write(&p, json).unwrap();
        let r = package_manager_pnpm_version(&p);
        let _ = std::fs::remove_file(&p);
        r
    }

    #[test]
    fn reads_the_version_this_repo_pins() {
        assert_eq!(parse(r#"{"packageManager":"pnpm@11.7.0"}"#).as_deref(), Some("11.7.0"));
    }

    /// corepack writes an integrity suffix when it pins; npm wants the bare version.
    #[test]
    fn drops_the_integrity_suffix() {
        assert_eq!(
            parse(r#"{"packageManager":"pnpm@10.4.1+sha512.abcdef"}"#).as_deref(),
            Some("10.4.1")
        );
    }

    /// Anything it cannot make sense of means "leave pnpm alone", never a failed install.
    #[test]
    fn anything_else_is_no_opinion() {
        assert_eq!(parse(r#"{"packageManager":"yarn@4.0.0"}"#), None);
        assert_eq!(parse(r#"{"name":"x"}"#), None);
        assert_eq!(parse("not json at all"), None);
        assert_eq!(parse(r#"{"packageManager":"pnpm@"}"#), None);
    }
}

#[cfg(test)]
mod ansi_tests {
    use super::strip_ansi;

    #[test]
    fn strips_the_colour_codes_vite_and_cargo_emit() {
        // Exactly the shape seen in the installer window.
        assert_eq!(
            strip_ansi("\u{1b}[32m✓\u{1b}[39m built in 3.39s"),
            "✓ built in 3.39s"
        );
        assert_eq!(
            strip_ansi("\u{1b}[2mdist/\u{1b}[22m\u{1b}[32mindex.html\u{1b}[39m"),
            "dist/index.html"
        );
        assert_eq!(
            strip_ansi("    \u{1b}[1m\u{1b}[32mFinished\u{1b}[0m release"),
            "    Finished release"
        );
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        assert_eq!(strip_ansi("Installed xconsole.exe"), "Installed xconsole.exe");
        assert_eq!(strip_ansi("Done — press Launch"), "Done — press Launch");
    }

    #[test]
    fn handles_osc_titles_and_progress_returns() {
        assert_eq!(strip_ansi("\u{1b}]0;building\u{7}done"), "done");
        assert_eq!(strip_ansi("\u{1b}]0;t\u{1b}\\after"), "after");
        assert_eq!(strip_ansi("50%\r100%"), "50%100%");
    }

    /// A cut-off escape at the end of a chunk must not swallow the text before it.
    #[test]
    fn a_truncated_escape_does_not_eat_the_line() {
        assert_eq!(strip_ansi("text\u{1b}"), "text");
        assert_eq!(strip_ansi("text\u{1b}["), "text");
    }
}

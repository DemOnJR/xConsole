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

use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

const REPO_URL: &str = "https://github.com/DemOnJR/xConsole";
const GNU_TOOLCHAIN: &str = "stable-x86_64-pc-windows-gnu";

// Portable toolchain downloads (only used when the tool isn't already on PATH).
const RUSTUP_URL: &str = "https://win.rustup.rs/x86_64";
const MINGIT_URL: &str =
    "https://github.com/git-for-windows/git/releases/download/v2.47.1.windows.1/MinGit-2.47.1-64-bit.zip";
const MINGW_URL: &str = "https://github.com/brechtsanders/winlibs_mingw/releases/download/14.2.0posix-19.1.1-12.0.0-ucrt-r3/winlibs-x86_64-posix-seh-gcc-14.2.0-llvm-19.1.1-mingw-w64ucrt-12.0.0-r3.zip";
const NODE_VER: &str = "v20.18.1";

const APP_NAME: &str = "xConsole";
const EXE_NAME: &str = "xconsole.exe";
const VERSION: &str = "0.1.0";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\xConsole";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

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
        }
    }
    fn log(&self, line: impl Into<String>) {
        let line = line.into();
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
    }
    fn check(&self, command_line: &str) -> bool {
        let mut c = Command::new("cmd");
        c.args(["/C", command_line]);
        self.apply(&mut c);
        c.creation_flags(CREATE_NO_WINDOW)
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
        c.creation_flags(CREATE_NO_WINDOW);
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

fn run_streamed(rep: &Reporter, mut cmd: Command, what: &str) -> Result<(), String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{what}: failed to start ({e})"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let r1 = rep.clone();
    let t1 = std::thread::spawn(move || {
        if let Some(o) = stdout {
            for line in BufReader::new(o).lines().map_while(Result::ok) {
                r1.log(line);
            }
        }
    });
    let r2 = rep.clone();
    let t2 = std::thread::spawn(move || {
        if let Some(e) = stderr {
            for line in BufReader::new(e).lines().map_while(Result::ok) {
                r2.log(line);
            }
        }
    });
    let status = child.wait().map_err(|e| format!("{what}: {e}"))?;
    let _ = t1.join();
    let _ = t2.join();
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed (exit {})", status.code().unwrap_or(-1)))
    }
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
    let full =
        resolve(program, &env.path).ok_or_else(|| format!("{program}: not found on PATH"))?;
    let is_script = full
        .extension()
        .map(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            e == "cmd" || e == "bat"
        })
        .unwrap_or(false);
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
    run_streamed(rep, c, what)
}

fn download(rep: &Reporter, url: &str, dest: &Path) -> Result<(), String> {
    let mut c = Command::new("powershell");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol='Tls12'; Invoke-WebRequest -Uri '{url}' -OutFile '{}'",
            dest.display()
        ),
    ]);
    run_streamed(rep, c, "download")
}

fn unzip(rep: &Reporter, zip: &Path, dest: &Path) -> Result<(), String> {
    let mut c = Command::new("powershell");
    c.args([
        "-NoProfile",
        "-Command",
        &format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            zip.display(),
            dest.display()
        ),
    ]);
    run_streamed(rep, c, "extract")
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
            unzip(rep, &zip, &base.join(r"tools\git"))?;
            let _ = std::fs::remove_file(&zip);
            if !current_env(&base).check("git --version") {
                return Err("Git did not install correctly.".into());
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
            let portable = BuildEnv {
                path: env.path.clone(),
                rustup_home: Some(base.join(".rustup")),
                cargo_home: Some(base.join(".cargo")),
            };
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
            run_streamed(rep, c, "rustup install")?;
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
            rep.log("pnpm is already available.");
        } else {
            rep.log("Enabling pnpm via corepack...");
            if run_tool(rep, &env, None, "corepack", &["enable", "pnpm"], "corepack").is_err() {
                run_tool(rep, &env, None, "npm", &["install", "-g", "pnpm"], "npm i -g pnpm")?;
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
            match install_webview2() {
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
        rep.log("Installing frontend dependencies (pnpm install)...");
        run_tool(rep, &env, Some(&src_dir()), "pnpm", &["install"], "pnpm install")?;
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
        run_tool(
            rep,
            &env,
            Some(&src),
            "pnpm",
            &["exec", "tauri", "build", "--no-bundle", "-c", ".xconsole-build.json"],
            "tauri build",
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
        if let Ok(me) = std::env::current_exe() {
            let _ = std::fs::copy(&me, base.join("uninstall.exe"));
        }
        register_uninstall(&base).map_err(|e| format!("registry: {e}"))?;
        rep.log("Registered in Apps & Features.");
        rep.log("Done — press Launch xConsole when you're ready.");
        Ok(())
    });

    Ok(())
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

fn install_webview2() -> Result<(), String> {
    let tmp = std::env::temp_dir().join("MicrosoftEdgeWebview2Setup.exe");
    let dl = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol='Tls12'; Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile '{}'",
                tmp.display()
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("download failed: {e}"))?;
    if !dl.success() {
        return Err("could not download the WebView2 bootstrapper".into());
    }
    let st = Command::new(&tmp)
        .args(["/silent", "/install"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("run bootstrapper: {e}"))?;
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
    let _ = Command::new("cmd")
        .args([
            "/C",
            &format!(
                "timeout /t 2 /nobreak >nul & rmdir /s /q \"{}\"",
                base.display()
            ),
        ])
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn();
}

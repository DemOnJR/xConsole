//! WhatsApp transport: a paired device, driven through a sidecar process.
//!
//! # Why a sidecar
//!
//! The onboarding the user actually wants is "scan a QR code" — no developer portal,
//! no bot, no token. That is WhatsApp's multi-device pairing, and speaking it means
//! implementing a Noise handshake, the Signal double ratchet, and WhatsApp's protobuf
//! dialect. No Rust crate does this. `whatsmeow` (Go) does, and is the library the
//! rest of the ecosystem is built on, so this module spawns a small Go binary and
//! talks to it in newline-delimited JSON over stdio.
//!
//! Stdio, not a socket, on purpose: a listening socket is exactly the inbound surface
//! the whole feature is designed not to have, and a pipe dies with its parent, so
//! closing xConsole cannot leave a paired WhatsApp session running behind it.
//!
//! # What lives where
//!
//! The sidecar owns the WhatsApp session (a SQLite store under the agent home) and
//! nothing else: no allowlist, no prefix, no idea what an agent is. Every
//! authorisation decision stays in [`super::authorize`], on this side of the pipe, so
//! a compromised or swapped sidecar can deliver messages but cannot authorise one.

use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use super::{Author, Config, IncomingMessage, Kind, Transport};

/// Event name the settings screen listens on for pairing progress.
pub const STATUS_EVENT: &str = "remote://whatsapp";

/// Settings key holding an explicit sidecar path, for installs that keep it somewhere
/// unusual. Empty means "look next to the executable".
pub const SETTING_SIDECAR: &str = "remote.whatsapp.sidecar_path";

/// How long a pairing attempt stays open before the sidecar is shut down again.
///
/// A QR that nobody scans should not leave a process running all day, and WhatsApp
/// rotates the code every twenty seconds anyway.
const LINK_WINDOW: std::time::Duration = std::time::Duration::from_secs(180);

/// What the settings screen needs to know about the link.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct WhatsAppStatus {
    /// The sidecar binary was found or ready.
    pub available: bool,
    /// Currently compiling or preparing the helper binary.
    pub building: bool,
    /// Progress step description (e.g. "Downloading Go compiler...", "Building WhatsApp helper...").
    pub build_step: Option<String>,
    pub running: bool,
    pub connected: bool,
    /// A device is paired. Survives restarts — the session lives on disk, so re-arming
    /// the bridge does not ask for another QR scan.
    pub linked: bool,
    pub jid: Option<String>,
    pub phone: Option<String>,
    pub push_name: Option<String>,
    /// The pairing QR, already rendered as SVG. Rendering here rather than in the
    /// webview keeps a QR library out of the frontend and means the code is drawn from
    /// the exact bytes the sidecar produced.
    pub qr_svg: Option<String>,
    pub error: Option<String>,
}

/// Everything the reader task, the transport and the settings commands share.
#[derive(Default)]
struct Shared {
    inbox: Mutex<VecDeque<IncomingMessage>>,
    status: Mutex<WhatsAppStatus>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    child: Mutex<Option<tokio::process::Child>>,
    /// Set while the user is on the pairing screen, so the driver does not shut the
    /// sidecar down underneath a QR code the bridge is not yet armed to use.
    linking: Mutex<Option<std::time::Instant>>,
}

/// One sidecar per process. The settings commands and the polling loop both need to
/// reach it, and two WhatsApp connections from one paired device is not a thing.
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn shared() -> &'static Arc<Shared> {
    SHARED.get_or_init(|| Arc::new(Shared::default()))
}

pub struct WhatsApp {
    app: tauri::AppHandle,
}

impl WhatsApp {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

// ---------------------------------------------------------------------------
// Locating the sidecar
// ---------------------------------------------------------------------------

const MAIN_GO: &str = include_str!("../../../sidecar/whatsapp/main.go");
const GO_MOD: &str = include_str!("../../../sidecar/whatsapp/go.mod");
const GO_SUM: &str = include_str!("../../../sidecar/whatsapp/go.sum");

const GO_DL_URL_WIN: &str = "https://go.dev/dl/go1.24.0.windows-amd64.zip";

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "xconsole-whatsapp.exe"
    } else {
        "xconsole-whatsapp"
    }
}

fn extract_zip(bytes: &[u8], dest: &std::path::Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("zip open: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let Some(rel) = f.enclosed_name() else { continue };
        let out = dest.join(rel);
        if f.is_dir() {
            let _ = std::fs::create_dir_all(&out);
        } else {
            if let Some(parent) = out.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut w = std::fs::File::create(&out).map_err(|e| format!("write {}: {e}", out.display()))?;
            std::io::copy(&mut f, &mut w).map_err(|e| format!("extract: {e}"))?;
        }
    }
    Ok(())
}

/// Where the sidecar might be, in order of preference.
///
/// The installed layout puts it beside the executable; a `cargo tauri dev` tree has it
/// where the build script wrote it. Checking both means the feature works while being
/// developed instead of only after packaging.
pub fn sidecar_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let db = app.state::<crate::storage::Db>();
    if let Some(explicit) = db
        .get_setting(SETTING_SIDECAR)
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        let p = std::path::PathBuf::from(explicit);
        return p.exists().then_some(p);
    }

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(binary_name()));
        }
    }
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join(binary_name()));
    }
    // Development tree: `sidecar/whatsapp/` next to Cargo.toml.
    candidates.push(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("sidecar")
            .join("whatsapp")
            .join(binary_name()),
    );
    // Agent home & user data paths
    let home = app.state::<crate::ai::AgentHome>().inner().0.clone();
    candidates.push(home.join("sidecar").join(binary_name()));
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let base = std::path::PathBuf::from(local_app_data).join("xConsole");
        candidates.push(base.join("app").join(binary_name()));
        candidates.push(base.join(r"src\src-tauri\sidecar\whatsapp").join(binary_name()));
        candidates.push(base.join("tools").join("whatsapp").join(binary_name()));
    }
    candidates.into_iter().find(|p| p.exists())
}

/// Where the paired session is kept.
///
/// Under the agent home rather than beside the binary, so an app update that replaces
/// the sidecar does not un-pair the user's phone.
fn store_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let home = app.state::<crate::ai::AgentHome>().inner().0.clone();
    let dir = home.join("whatsapp");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("session.db")
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Current link state, for the settings screen.
pub async fn status(app: &tauri::AppHandle) -> WhatsAppStatus {
    let mut s = shared().status.lock().await.clone();
    s.available = sidecar_path(app).is_some();
    s.running = shared().child.lock().await.is_some();
    s
}

async fn update_status(app: &tauri::AppHandle, f: impl FnOnce(&mut WhatsAppStatus)) {
    {
        let mut s = shared().status.lock().await;
        f(&mut s);
    }
    let snapshot = status(app).await;
    let _ = app.emit(STATUS_EVENT, snapshot);
}

/// Locate or install the Go compiler toolchain.
async fn locate_or_install_go(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    // 1. Check if `go` is on PATH and working.
    if let Ok(out) = crate::proc::quiet_command("go").arg("version").output() {
        if out.status.success() {
            return Ok(std::path::PathBuf::from("go"));
        }
    }

    // 2. Check candidate local paths (e.g. %LOCALAPPDATA%\xConsole\tools\go\bin\go.exe or AgentHome/tools/go/bin/go.exe).
    let mut candidate_go_bins = Vec::new();
    let home = app.state::<crate::ai::AgentHome>().inner().0.clone();
    let home_go_bin = home
        .join("tools")
        .join("go")
        .join("bin")
        .join(if cfg!(windows) { "go.exe" } else { "go" });
    candidate_go_bins.push(home_go_bin);

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let p = std::path::PathBuf::from(local_app_data)
            .join("xConsole")
            .join("tools")
            .join("go")
            .join("bin")
            .join(if cfg!(windows) { "go.exe" } else { "go" });
        candidate_go_bins.push(p);
    }

    for bin in candidate_go_bins {
        if bin.exists() {
            if let Ok(out) = crate::proc::quiet_command(&bin).arg("version").output() {
                if out.status.success() {
                    return Ok(bin);
                }
            }
        }
    }

    // 3. If Windows, auto-download portable Go into home.join("tools").
    if cfg!(windows) {
        update_status(app, |s| {
            s.building = true;
            s.build_step = Some("Downloading portable Go compiler (~80MB)…".into());
        })
        .await;

        let tools_dir = home.join("tools");
        let _ = std::fs::create_dir_all(&tools_dir);

        let client = reqwest::Client::builder()
            .user_agent("xConsole/1.0")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;

        let bytes = client
            .get(GO_DL_URL_WIN)
            .send()
            .await
            .map_err(|e| format!("failed to download Go toolchain: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Go download returned error: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("reading Go download stream: {e}"))?;

        update_status(app, |s| {
            s.build_step = Some("Extracting Go toolchain…".into());
        })
        .await;

        extract_zip(&bytes, &tools_dir)?;

        let downloaded_bin = tools_dir.join("go").join("bin").join("go.exe");
        if downloaded_bin.exists() {
            return Ok(downloaded_bin);
        }
    }

    Err("Go compiler is not installed. Please install Go or check your internet connection.".into())
}

/// Automatically compiles and installs the WhatsApp sidecar binary.
pub async fn ensure_sidecar_installed(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    if let Some(existing) = sidecar_path(app) {
        return Ok(existing);
    }

    update_status(app, |s| {
        s.building = true;
        s.build_step = Some("Preparing Go compiler…".into());
        s.error = None;
    })
    .await;

    let go_bin = match locate_or_install_go(app).await {
        Ok(b) => b,
        Err(e) => {
            update_status(app, |s| {
                s.building = false;
                s.build_step = None;
                s.error = Some(e.clone());
            })
            .await;
            return Err(e);
        }
    };

    update_status(app, |s| {
        s.build_step = Some("Preparing WhatsApp helper sources…".into());
    })
    .await;

    let home = app.state::<crate::ai::AgentHome>().inner().0.clone();

    // Choose source dir: dev repo if present, otherwise write embedded files into home/sidecar_src/whatsapp
    let dev_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("sidecar")
        .join("whatsapp");
    let src_dir = if dev_src.join("main.go").exists() {
        dev_src.clone()
    } else {
        let embedded_dir = home.join("sidecar_src").join("whatsapp");
        let _ = std::fs::create_dir_all(&embedded_dir);
        let _ = std::fs::write(embedded_dir.join("main.go"), MAIN_GO);
        let _ = std::fs::write(embedded_dir.join("go.mod"), GO_MOD);
        let _ = std::fs::write(embedded_dir.join("go.sum"), GO_SUM);
        embedded_dir
    };

    // Target destination
    let dest_dir = if src_dir == dev_src {
        src_dir.clone()
    } else {
        let d = home.join("sidecar");
        let _ = std::fs::create_dir_all(&d);
        d
    };
    let out_bin = dest_dir.join(binary_name());

    update_status(app, |s| {
        s.build_step = Some("Compiling WhatsApp helper (xconsole-whatsapp)…".into());
    })
    .await;

    let mut cmd = crate::proc::quiet_tokio(&go_bin);
    cmd.current_dir(&src_dir);
    cmd.arg("build")
        .arg("-trimpath")
        .arg("-ldflags=-s -w")
        .arg("-o")
        .arg(&out_bin)
        .arg(".");

    cmd.env("CGO_ENABLED", "0");
    if cfg!(windows) {
        cmd.env("GOOS", "windows");
        cmd.env("GOARCH", "amd64");
    } else if cfg!(target_os = "macos") {
        cmd.env("GOOS", "darwin");
        if cfg!(target_arch = "aarch64") {
            cmd.env("GOARCH", "arm64");
        } else {
            cmd.env("GOARCH", "amd64");
        }
    } else {
        cmd.env("GOOS", "linux");
        cmd.env("GOARCH", "amd64");
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("failed to run go build: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let err_msg = format!("Go compilation failed: {}", stderr.trim());
        update_status(app, |s| {
            s.building = false;
            s.build_step = None;
            s.error = Some(err_msg.clone());
        })
        .await;
        return Err(err_msg);
    }

    if !out_bin.exists() {
        let err_msg = format!("Go build succeeded but {} was not created", out_bin.display());
        update_status(app, |s| {
            s.building = false;
            s.build_step = None;
            s.error = Some(err_msg.clone());
        })
        .await;
        return Err(err_msg);
    }

    let db = app.state::<crate::storage::Db>();
    let _ = db.set_setting(SETTING_SIDECAR, &out_bin.to_string_lossy());

    update_status(app, |s| {
        s.available = true;
        s.building = false;
        s.build_step = None;
        s.error = None;
    })
    .await;

    Ok(out_bin)
}

pub async fn auto_install(app: &tauri::AppHandle) -> Result<WhatsAppStatus, String> {
    ensure_sidecar_installed(app).await?;
    Ok(status(app).await)
}

/// Draw a pairing code as an SVG the webview can drop straight into the DOM.
fn qr_svg(code: &str) -> Option<String> {
    use qrcode::render::svg;
    // Medium correction: the code is displayed on a screen, not printed, so the
    // trade-off is toward a denser-but-smaller image being scannable at arm's length.
    let qr = qrcode::QrCode::with_error_correction_level(code, qrcode::EcLevel::M).ok()?;
    Some(
        qr.render::<svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            // Fixed colours, not theme tokens: a QR scanner needs real contrast, and a
            // dark-on-dark code is unreadable no matter how good it looks.
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

// ---------------------------------------------------------------------------
// Sidecar lifecycle
// ---------------------------------------------------------------------------

/// Start the sidecar if it is not already running. Idempotent.
pub async fn ensure_running(app: &tauri::AppHandle) -> Result<(), String> {
    if shared().child.lock().await.is_some() {
        return Ok(());
    }
    let path = match sidecar_path(app) {
        Some(p) => p,
        None => ensure_sidecar_installed(app).await?,
    };

    let mut cmd = crate::proc::quiet_tokio(path);
    cmd.arg("--store")
        .arg(store_path(app))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not start the WhatsApp helper: {e}"))?;

    let stdout = child.stdout.take().ok_or("the WhatsApp helper produced no output stream")?;
    let stderr = child.stderr.take();
    *shared().stdin.lock().await = child.stdin.take();
    *shared().child.lock().await = Some(child);

    // The sidecar's own logs are diagnostics, not events. Draining them matters even
    // when nothing reads them: a full stderr pipe blocks the writer, and a blocked
    // sidecar stops delivering messages.
    if let Some(stderr) = stderr {
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                crate::diag(&format!("remote(whatsapp): {line}"));
            }
        });
    }

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(ev) = serde_json::from_str::<serde_json::Value>(&line) {
                handle_event(&app2, ev).await;
            }
        }
        // The pipe closed: the sidecar exited. Clear the handles so the next poll
        // starts a fresh one rather than writing into a dead stdin forever.
        *shared().stdin.lock().await = None;
        *shared().child.lock().await = None;
        update_status(&app2, |s| {
            s.running = false;
            s.connected = false;
            s.qr_svg = None;
        })
        .await;
    });

    update_status(app, |s| {
        s.running = true;
        s.error = None;
    })
    .await;
    Ok(())
}

/// Stop the sidecar and forget anything queued.
pub async fn stop() {
    *shared().stdin.lock().await = None;
    if let Some(mut child) = shared().child.lock().await.take() {
        let _ = child.kill().await;
    }
    shared().inbox.lock().await.clear();
    let mut s = shared().status.lock().await;
    s.running = false;
    s.connected = false;
    s.qr_svg = None;
}

async fn send_command(cmd: serde_json::Value) -> Result<(), String> {
    let mut guard = shared().stdin.lock().await;
    let stdin = guard.as_mut().ok_or("the WhatsApp helper is not running")?;
    let mut line = serde_json::to_string(&cmd).map_err(|e| e.to_string())?;
    line.push('\n');
    stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

/// Write to one chat, from anywhere.
///
/// The driver is not the only sender any more: a conversation can be moved to WhatsApp
/// from another transport, and the agent can report a finished job unprompted. Both need
/// to reach a chat without owning the `Transport` the driver loop is holding. Requires a
/// running sidecar, which an armed bridge already keeps up — an unarmed one says so
/// rather than dropping the message.
pub async fn send_message(chat_id: &str, text: &str) -> Result<(), String> {
    for chunk in super::chunk_for(Kind::WhatsApp, text) {
        send_command(serde_json::json!({
            "type": "send",
            "chat": chat_id,
            "text": chunk,
        }))
        .await?;
    }
    Ok(())
}

/// Begin (or resume) pairing, and keep the sidecar up long enough to scan.
pub async fn link_start(app: &tauri::AppHandle) -> Result<WhatsAppStatus, String> {
    *shared().linking.lock().await = Some(std::time::Instant::now());
    ensure_running(app).await?;
    Ok(status(app).await)
}

/// Cancel a pairing attempt the user walked away from.
pub async fn link_cancel(app: &tauri::AppHandle) -> WhatsAppStatus {
    *shared().linking.lock().await = None;
    let armed = super::load_config(&app.state::<crate::storage::Db>(), Kind::WhatsApp).is_usable();
    if !armed {
        stop().await;
    }
    update_status(app, |s| s.qr_svg = None).await;
    status(app).await
}

/// Unpair the phone: tell WhatsApp to drop the device, then delete the local session.
///
/// Both halves matter. Logging out without deleting leaves a store that reconnects on
/// the next launch; deleting without logging out leaves a stale linked device on the
/// user's phone that they then have to hunt down in WhatsApp's own settings.
pub async fn unlink(app: &tauri::AppHandle) -> Result<WhatsAppStatus, String> {
    if shared().child.lock().await.is_some() {
        let _ = send_command(serde_json::json!({ "type": "logout" })).await;
        // Give the sidecar a moment to send the logout before the pipe is cut.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    stop().await;
    let _ = std::fs::remove_file(store_path(app));
    *shared().linking.lock().await = None;
    update_status(app, |s| {
        s.linked = false;
        s.connected = false;
        s.jid = None;
        s.phone = None;
        s.push_name = None;
        s.qr_svg = None;
    })
    .await;
    Ok(status(app).await)
}

// ---------------------------------------------------------------------------
// Sidecar events
// ---------------------------------------------------------------------------

/// The user part of a JID: `40712345678:3@s.whatsapp.net` -> `40712345678`.
fn jid_user(jid: &str) -> String {
    jid.split('@').next().unwrap_or(jid).split(':').next().unwrap_or(jid).to_string()
}

async fn handle_event(app: &tauri::AppHandle, ev: serde_json::Value) {
    let kind = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match kind {
        "qr" => {
            let svg = ev.get("code").and_then(|c| c.as_str()).and_then(qr_svg);
            update_status(app, |s| {
                s.qr_svg = svg;
                s.error = None;
            })
            .await;
        }
        "paired" | "connected" => {
            let jid = ev.get("jid").and_then(|j| j.as_str()).unwrap_or("").to_string();
            let push = ev.get("push_name").and_then(|j| j.as_str()).map(str::to_string);
            *shared().linking.lock().await = None;
            update_status(app, |s| {
                s.linked = true;
                s.connected = true;
                // The scan succeeded, so the code has served its purpose. Leaving it on
                // screen invites a second device being paired to the same account.
                s.qr_svg = None;
                s.error = None;
                if !jid.is_empty() {
                    s.phone = Some(jid_user(&jid));
                    s.jid = Some(jid.clone());
                }
                if push.is_some() {
                    s.push_name = push.clone();
                }
            })
            .await;
        }
        "disconnected" => {
            update_status(app, |s| s.connected = false).await;
        }
        "logged_out" => {
            // The phone unpaired us from WhatsApp's side. Say so plainly; the
            // alternative is a bridge that is armed in settings and silently deaf.
            update_status(app, |s| {
                s.linked = false;
                s.connected = false;
                s.jid = None;
                s.phone = None;
                s.error = Some("WhatsApp unlinked this device — scan again to reconnect".into());
            })
            .await;
        }
        "error" => {
            let msg = ev.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
            crate::diag(&format!("remote(whatsapp): {msg}"));
            update_status(app, |s| s.error = Some(msg.to_string())).await;
        }
        "message" => {
            if let Some(msg) = parse_message(&ev) {
                shared().inbox.lock().await.push_back(msg);
            }
        }
        _ => {}
    }
}

/// Normalise a sidecar message event.
///
/// Returns `None` for anything the bridge cannot act on, rather than inventing empty
/// fields — a message with no sender must never reach `authorize`, where a blank id
/// could be compared against a blank allowlist entry.
fn parse_message(ev: &serde_json::Value) -> Option<IncomingMessage> {
    let text = ev.get("text").and_then(|t| t.as_str()).unwrap_or("");
    if text.trim().is_empty() {
        return None;
    }
    let sender = ev.get("sender_id").and_then(|s| s.as_str()).unwrap_or("").trim();
    if sender.is_empty() {
        return None;
    }
    let chat = ev.get("chat").and_then(|c| c.as_str()).unwrap_or("").trim();
    Some(IncomingMessage {
        id: ev.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string(),
        chat_id: jid_user(chat),
        author: Author {
            id: jid_user(sender),
            username: ev
                .get("sender_username")
                .and_then(|u| u.as_str())
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(str::to_string),
            display_name: ev
                .get("push_name")
                .and_then(|p| p.as_str())
                .filter(|p| !p.trim().is_empty())
                .unwrap_or("someone")
                .to_string(),
        },
        // Our own outgoing messages come back on the same stream. Treating them as bot
        // messages reuses the loop guard the other transports already rely on.
        is_bot: ev.get("from_me").and_then(|f| f.as_bool()).unwrap_or(false),
        content: text.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl Transport for WhatsApp {
    fn kind(&self) -> Kind {
        Kind::WhatsApp
    }

    async fn poll(&mut self, _cfg: &Config) -> Result<Vec<IncomingMessage>, String> {
        ensure_running(&self.app).await?;
        let mut inbox = shared().inbox.lock().await;
        Ok(inbox.drain(..).collect())
    }

    async fn send(&mut self, _cfg: &Config, to: &IncomingMessage, text: &str) -> Result<(), String> {
        send_message(&to.chat_id, text).await
    }

    fn reset(&mut self) {
        // The bridge is off. Shut the sidecar down so a disabled transport is not
        // quietly holding a live WhatsApp session — unless the user is mid-scan, in
        // which case the config is *expected* to be unusable and pulling the process
        // would kill the QR they are looking at.
        tauri::async_runtime::spawn(async move {
            let linking = *shared().linking.lock().await;
            if let Some(started) = linking {
                if started.elapsed() < LINK_WINDOW {
                    return;
                }
                *shared().linking.lock().await = None;
            }
            if shared().child.lock().await.is_some() {
                stop().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jid_reduces_to_the_number_the_user_would_type() {
        // whatsmeow reports `<number>:<device>@s.whatsapp.net`. The allowlist is typed
        // from a contacts app, so the device suffix has to go or nothing ever matches.
        assert_eq!(jid_user("40712345678:3@s.whatsapp.net"), "40712345678");
        assert_eq!(jid_user("40712345678@s.whatsapp.net"), "40712345678");
        assert_eq!(jid_user("40712345678"), "40712345678");
        assert_eq!(jid_user("120363000000000000@g.us"), "120363000000000000");
    }

    #[test]
    fn a_message_with_no_sender_is_dropped_rather_than_blanked() {
        // A blank id reaching `authorize` could be matched by a blank allowlist entry.
        // `parse_id_list` already strips those, but the two guards are cheap and the
        // failure they prevent is total.
        assert!(parse_message(&serde_json::json!({"text": "hi", "chat": "1@s.whatsapp.net"})).is_none());
        assert!(parse_message(&serde_json::json!({"text": "hi", "sender_id": "  "})).is_none());
    }

    #[test]
    fn empty_messages_never_reach_the_agent() {
        // Images, reactions and receipts all arrive with no text.
        assert!(parse_message(&serde_json::json!({
            "sender_id": "40712345678@s.whatsapp.net", "text": "   "
        }))
        .is_none());
    }

    #[test]
    fn our_own_replies_come_back_marked_so_the_loop_guard_catches_them() {
        // WhatsApp echoes outgoing messages to linked devices. Without this the agent's
        // own reply would be read as the next command.
        let m = parse_message(&serde_json::json!({
            "id": "A1",
            "chat": "40712345678@s.whatsapp.net",
            "sender_id": "40799999999:2@s.whatsapp.net",
            "push_name": "Ada",
            "from_me": true,
            "text": "done"
        }))
        .unwrap();
        assert!(m.is_bot);
        assert_eq!(m.author.id, "40799999999");
        assert_eq!(m.chat_id, "40712345678");
    }

    #[test]
    fn a_username_is_carried_when_whatsapp_reports_one() {
        let m = parse_message(&serde_json::json!({
            "id": "A2",
            "chat": "40712345678@s.whatsapp.net",
            "sender_id": "40712345678@s.whatsapp.net",
            "sender_username": "ada.lovelace",
            "text": "!x uptime"
        }))
        .unwrap();
        assert_eq!(m.author.username.as_deref(), Some("ada.lovelace"));
        // Absent or blank usernames stay absent rather than becoming an empty string
        // that an allowlist entry of `@` could match.
        let m2 = parse_message(&serde_json::json!({
            "sender_id": "1@s.whatsapp.net", "sender_username": "  ", "text": "hi"
        }))
        .unwrap();
        assert_eq!(m2.author.username, None);
    }

    #[test]
    fn a_pairing_code_renders_to_something_a_phone_can_read() {
        let svg = qr_svg("2@abcdefghijklmnop/qrstuvwxyz+0123456789=,AbCdEf").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("#000000"));
    }
}

//! Speech-to-text for the voice pipeline. Local transcription shells out to
//! whisper.cpp's `whisper-cli` (no in-app C++/bindgen build — same install model
//! as the llama.cpp server); cloud transcription posts to an OpenAI-compatible
//! `/audio/transcriptions` endpoint using a configured OpenAI provider's key.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::ai::providers::join_url;
use crate::ai::web_tools::http_client;
use crate::secrets;
use crate::storage::Db;

const WHISPER_ZIP_URL: &str =
    "https://github.com/ggml-org/whisper.cpp/releases/latest/download/whisper-bin-x64.zip";
// Medium multilingual model (~1.5 GB) — best accuracy for non-English (e.g. Romanian).
// Slower than small/base on CPU; the user opted for accuracy over STT speed.
const WHISPER_MODEL_FILE: &str = "ggml-medium.bin";
const WHISPER_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin";

/// Download a whisper GGML model by filename from the official whisper.cpp repo.
pub async fn download_whisper_model(app: &AppHandle, model_file: &str) -> Result<String, String> {
    let file = model_file.trim();
    if !file.starts_with("ggml-") || !file.ends_with(".bin") {
        return Err("model must be a ggml-*.bin whisper file".into());
    }
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{file}");
    crate::ai::models::download_gguf(app, "whisper-model", &url, file).await?;
    Ok(file.to_string())
}

fn whisper_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"))
        .join("whisper")
}

fn emit_setup(app: &AppHandle, status: &str, message: &str) {
    let _ = app.emit(
        "voice://whisper-setup",
        serde_json::json!({ "status": status, "message": message }),
    );
}

/// Recursively find a file named `name` under `dir`.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(found) = find_file(&p, name) {
                return Some(found);
            }
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(p);
        }
    }
    None
}

/// One-click local-voice setup (Windows): download the prebuilt whisper.cpp,
/// extract `whisper-cli.exe`, fetch a base model, and return the binary path so
/// the caller can store it as `whisper.bin_path`.
pub async fn setup_whisper(app: &AppHandle) -> Result<String, String> {
    if !cfg!(windows) {
        return Err(
            "Auto-setup is Windows-only. On macOS/Linux install whisper.cpp (brew/package manager) \
             so `whisper-cli` is on PATH, or use the Cloud STT engine."
                .into(),
        );
    }
    emit_setup(app, "downloading", "Downloading whisper.cpp…");
    // No-timeout client — the zip is tens of MB.
    let client = reqwest::Client::builder()
        .user_agent("xConsole/1.0")
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(WHISPER_ZIP_URL)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("reading download: {e}"))?;

    emit_setup(app, "extracting", "Extracting…");
    let dir = whisper_dir(app);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    extract_zip(&bytes, &dir)?;
    let bin = find_file(&dir, "whisper-cli.exe")
        .ok_or("whisper-cli.exe not found in the downloaded archive")?;

    emit_setup(app, "model", "Downloading speech model…");
    crate::ai::models::download_gguf(app, "whisper-model", WHISPER_MODEL_URL, WHISPER_MODEL_FILE).await?;

    emit_setup(app, "done", "Local voice ready.");
    Ok(bin.to_string_lossy().to_string())
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("zip open: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let Some(rel) = f.enclosed_name() else { continue };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).ok();
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut w = std::fs::File::create(&out).map_err(|e| format!("write {}: {e}", out.display()))?;
            std::io::copy(&mut f, &mut w).map_err(|e| format!("extract: {e}"))?;
        }
    }
    Ok(())
}

/// The default whisper model filename the auto-setup installs.
pub fn default_whisper_model() -> &'static str {
    WHISPER_MODEL_FILE
}

/// Locate `whisper-server` — next to the configured whisper-cli, else on PATH.
pub fn find_whisper_server(cli_path: Option<&str>) -> Option<String> {
    let exe = if cfg!(windows) { "whisper-server.exe" } else { "whisper-server" };
    if let Some(p) = cli_path.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(dir) = Path::new(p).parent() {
            let cand = dir.join(exe);
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    let finder = if cfg!(windows) { "where" } else { "which" };
    if let Ok(out) = crate::proc::quiet_command(finder).arg(exe).output() {
        if out.status.success() {
            if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let p = line.trim();
                if !p.is_empty() {
                    return Some(p.to_string());
                }
            }
        }
    }
    None
}

// A managed whisper.cpp HTTP server. Spawned once with the model warm, so live
// conversation turns transcribe near-instantly. Spawning a long-lived server with
// null stdio + CREATE_NO_WINDOW (matching the app's other subprocesses) also avoids
// the per-process console-init crash (STATUS_DLL_INIT_FAILED / 0xC0000142) seen
// when capturing stdout from a fresh whisper-cli process on every utterance.
struct RunningWhisper {
    child: tokio::process::Child,
    model: String,
    base: String,
}

fn whisper_state() -> &'static Mutex<Option<RunningWhisper>> {
    static WHISPER: OnceLock<Mutex<Option<RunningWhisper>>> = OnceLock::new();
    WHISPER.get_or_init(|| Mutex::new(None))
}

fn pick_port(start: u16) -> u16 {
    for p in start..start + 50 {
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
    start
}

/// Ensure a whisper-server is running for `model_file`; returns its base URL.
async fn ensure_whisper(app: &AppHandle, db: &Db, model_file: &str) -> Result<String, String> {
    if model_file.trim().is_empty() {
        return Err(
            "No whisper model selected. Use Settings → Voice → set up local voice, \
             or switch to the Cloud engine."
                .into(),
        );
    }
    let model = crate::ai::models::models_dir(app).join(model_file);
    if !model.exists() {
        return Err(format!("whisper model not found: {}", model.display()));
    }

    let mut guard = whisper_state().lock().await;
    if let Some(r) = guard.as_mut() {
        if r.model == model_file && matches!(r.child.try_wait(), Ok(None)) {
            return Ok(r.base.clone());
        }
        let _ = r.child.start_kill();
        *guard = None;
    }

    let cli = db.get_setting("whisper.bin_path").ok().flatten();
    let bin = find_whisper_server(cli.as_deref()).ok_or(
        "whisper-server not found. Use Settings → Voice → set up local voice, \
         or switch to the Cloud engine.",
    )?;
    let parent = Path::new(&bin).parent().map(|p| p.to_path_buf());
    let port = pick_port(8765);
    let base = format!("http://127.0.0.1:{port}");

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("-m")
        .arg(&model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-nt")
        .arg("-l")
        .arg("auto")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(p) = &parent {
        cmd.current_dir(p);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let child = cmd.spawn().map_err(|e| format!("starting whisper-server: {e}"))?;
    *guard = Some(RunningWhisper {
        child,
        model: model_file.to_string(),
        base: base.clone(),
    });

    // Health-poll: the server answers HTTP once the model has loaded.
    let client = http_client()?;
    for _ in 0..80 {
        if let Some(r) = guard.as_mut() {
            if let Ok(Some(status)) = r.child.try_wait() {
                let code = status.code();
                *guard = None;
                return Err(format!(
                    "whisper-server exited during startup (code {code:?}). \
                     Try Settings → Voice → set up local voice again, or use the Cloud engine."
                ));
            }
        }
        if client
            .get(format!("{base}/"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .is_ok()
        {
            return Ok(base);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("whisper-server did not become ready in time.".into())
}

/// Transcribe a 16 kHz mono WAV locally via the managed whisper-server.
/// `lang` is an ISO code ("ro", "en", …) or "auto" to auto-detect.
pub async fn transcribe_local(
    app: &AppHandle,
    db: &Db,
    wav: &[u8],
    model_file: &str,
    lang: &str,
) -> Result<String, String> {
    let base = ensure_whisper(app, db, model_file).await?;
    let url = format!("{base}/inference");
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let lang = if lang.trim().is_empty() { "auto" } else { lang.trim() };
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("response_format", "json")
        .text("language", lang.to_string())
        .text("temperature", "0.0");
    let client = http_client()?;
    let resp = client
        .post(&url)
        .multipart(form)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("whisper-server request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("whisper-server HTTP {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
            return Ok(t.trim().to_string());
        }
    }
    Ok(body.trim().to_string())
}

/// Transcribe via an OpenAI-compatible endpoint. `base_match`: if Some, pick the
/// enabled `openai`-kind provider whose base URL contains it (e.g. "groq"), so the
/// user can run a free Groq Whisper provider alongside a paid OpenAI one. `model`
/// is the transcription model id for that endpoint.
async fn transcribe_openai_compat(
    db: &Db,
    wav: &[u8],
    lang: &str,
    base_match: Option<&str>,
    model: &str,
) -> Result<String, String> {
    let provider = db
        .list_providers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| {
            p.enabled
                && p.kind == "openai"
                && base_match.map_or(true, |m| {
                    p.base_url.as_deref().unwrap_or("").to_lowercase().contains(m)
                })
        })
        .ok_or_else(|| match base_match {
            Some("groq") => "No Groq provider found. Add one (free) in Settings → Providers (Quick add → Groq) and enable it.".to_string(),
            _ => "Cloud transcription needs an enabled OpenAI provider (Settings → Providers).".to_string(),
        })?;
    let key = secrets::get_secret(&secrets::provider_key(&provider.id))
        .ok()
        .flatten()
        .map(|z| z.to_string())
        .ok_or("That provider has no API key set.")?;
    let base = provider
        .base_url
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.openai.com/v1".into());
    let url = join_url(&base, "audio/transcriptions");

    let client = http_client()?;
    let part = reqwest::multipart::Part::bytes(wav.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model.to_string());
    let lang = lang.trim();
    if !lang.is_empty() && lang != "auto" {
        form = form.text("language", lang.to_string());
    }
    let resp = client
        .post(url)
        .bearer_auth(&key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("cloud transcription failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        return Err(format!("cloud transcription HTTP {status}: {}", detail.chars().take(160).collect::<String>()));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("cloud transcription parse: {e}"))?;
    Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string())
}

/// Cloud STT via a configured OpenAI provider (`whisper-1`).
pub async fn transcribe_cloud(db: &Db, wav: &[u8], lang: &str) -> Result<String, String> {
    transcribe_openai_compat(db, wav, lang, None, "whisper-1").await
}

/// Free cloud STT via a configured Groq provider (`whisper-large-v3`).
pub async fn transcribe_groq(db: &Db, wav: &[u8], lang: &str) -> Result<String, String> {
    transcribe_openai_compat(db, wav, lang, Some("groq"), "whisper-large-v3").await
}

/// Natural text-to-speech via OpenAI's /audio/speech (gpt-4o-mini-tts), reusing a
/// configured OpenAI provider's key. Returns WAV bytes. Far more natural than the
/// robotic SAPI voices the webview's Web Speech API exposes.
pub async fn synthesize_cloud(
    db: &Db,
    text: &str,
    voice: &str,
    instructions: &str,
) -> Result<Vec<u8>, String> {
    let provider = db
        .list_providers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.enabled && p.kind == "openai")
        .ok_or("Natural voices need an enabled OpenAI provider (Settings → Providers).")?;
    let key = secrets::get_secret(&secrets::provider_key(&provider.id))
        .ok()
        .flatten()
        .map(|z| z.to_string())
        .ok_or("The OpenAI provider has no API key set.")?;
    let base = provider
        .base_url
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.openai.com/v1".into());
    let url = join_url(&base, "audio/speech");

    let voice = if voice.trim().is_empty() { "sage" } else { voice.trim() };
    let instructions = if instructions.trim().is_empty() {
        "Warm, calm, conversational, and concise."
    } else {
        instructions.trim()
    };
    let body = serde_json::json!({
        "model": "gpt-4o-mini-tts",
        "input": text,
        "voice": voice,
        "response_format": "wav",
        "instructions": instructions,
    });
    let client = http_client()?;
    let resp = client
        .post(url)
        .bearer_auth(&key)
        .json(&body)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("speech synthesis failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let detail = resp.text().await.unwrap_or_default();
        return Err(format!("speech synthesis HTTP {status}: {}", detail.chars().take(200).collect::<String>()));
    }
    Ok(resp
        .bytes()
        .await
        .map_err(|e| format!("reading audio: {e}"))?
        .to_vec())
}

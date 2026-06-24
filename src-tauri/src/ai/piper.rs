//! Offline neural text-to-speech via Piper (rhasspy/piper). Kokoro has no prebuilt
//! binary and needs GPL espeak-ng built under MinGW (a non-starter here); Piper ships
//! a self-contained Windows binary that bundles espeak-ng-data + onnxruntime, so we
//! manage it as a sidecar exactly like whisper.cpp/llama.cpp: download once, then run
//! it per utterance (text on stdin → WAV file) and return the audio bytes.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};

use crate::storage::Db;

const PIPER_ZIP_URL: &str =
    "https://github.com/rhasspy/piper/releases/latest/download/piper_windows_amd64.zip";

/// Default voice installed by setup.
pub const DEFAULT_VOICE: &str = "en_US-amy-medium";

/// Curated voices → their subpath in the rhasspy/piper-voices HF repo.
pub fn voice_subpath(key: &str) -> Option<&'static str> {
    Some(match key {
        "en_US-amy-medium" => "en/en_US/amy/medium",
        "en_US-ryan-high" => "en/en_US/ryan/high",
        "en_US-hfc_female-medium" => "en/en_US/hfc_female/medium",
        "en_GB-alba-medium" => "en/en_GB/alba/medium",
        "ro_RO-mihai-medium" => "ro/ro_RO/mihai/medium",
        "de_DE-thorsten-medium" => "de/de_DE/thorsten/medium",
        "es_ES-davefx-medium" => "es/es_ES/davefx/medium",
        "fr_FR-siwis-medium" => "fr/fr_FR/siwis/medium",
        "it_IT-riccardo-x_low" => "it/it_IT/riccardo/x_low",
        _ => return None,
    })
}

fn piper_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"))
        .join("piper")
}

fn emit(app: &AppHandle, status: &str, message: &str) {
    let _ = app.emit(
        "voice://piper-setup",
        serde_json::json!({ "status": status, "message": message }),
    );
}

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

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("zip open: {e}"))?;
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

/// Locate the piper binary (managed install, configured path, or PATH).
pub fn find_piper(app: &AppHandle, override_path: Option<&str>) -> Option<String> {
    let exe = if cfg!(windows) { "piper.exe" } else { "piper" };
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        if Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    if let Some(found) = find_file(&piper_dir(app), exe) {
        return Some(found.to_string_lossy().into_owned());
    }
    let finder = if cfg!(windows) { "where" } else { "which" };
    if let Ok(out) = std::process::Command::new(finder).arg(exe).output() {
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

/// Download the prebuilt Piper (Windows) + the default voice. Returns the binary path.
pub async fn setup_piper(app: &AppHandle) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("Auto-install is Windows-only. Install Piper and set piper.bin_path.".into());
    }
    emit(app, "downloading", "Downloading Piper…");
    let client = reqwest::Client::builder()
        .user_agent("xConsole/1.0")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())?;
    let bytes = client
        .get(PIPER_ZIP_URL)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("download HTTP error: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("reading download: {e}"))?;

    emit(app, "extracting", "Extracting…");
    let dir = piper_dir(app);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    extract_zip(&bytes, &dir)?;
    let bin = find_file(&dir, "piper.exe").ok_or("piper.exe not found in the downloaded archive")?;

    emit(app, "voice", "Downloading the default voice…");
    download_voice(app, DEFAULT_VOICE).await?;

    emit(app, "done", "Offline voice ready.");
    Ok(bin.to_string_lossy().into_owned())
}

/// Download a Piper voice (`.onnx` + `.onnx.json`) into the models dir.
pub async fn download_voice(app: &AppHandle, key: &str) -> Result<String, String> {
    let sub = voice_subpath(key).ok_or_else(|| format!("unknown Piper voice: {key}"))?;
    let base = format!("https://huggingface.co/rhasspy/piper-voices/resolve/main/{sub}/{key}");
    crate::ai::models::download_gguf(app, "piper-voice", &format!("{base}.onnx"), &format!("{key}.onnx")).await?;
    crate::ai::models::download_gguf(app, "piper-voice", &format!("{base}.onnx.json"), &format!("{key}.onnx.json")).await?;
    Ok(key.to_string())
}

/// Synthesize `text` to WAV bytes with a local Piper voice.
pub async fn synthesize_local_piper(
    app: &AppHandle,
    db: &Db,
    text: &str,
    voice: &str,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncWriteExt;

    let voice = if voice.trim().is_empty() { DEFAULT_VOICE } else { voice.trim() };
    let bin = find_piper(app, db.get_setting("piper.bin_path").ok().flatten().as_deref()).ok_or(
        "Piper not installed. Use Settings → Voice → set up offline voice, or switch the TTS engine.",
    )?;
    let model = crate::ai::models::models_dir(app).join(format!("{voice}.onnx"));
    if !model.exists() {
        return Err(format!(
            "voice not downloaded: {voice}. Pick it in Settings → Voice and download it."
        ));
    }
    let tmp = std::env::temp_dir().join(format!("xconsole_tts_{}.wav", uuid::Uuid::new_v4()));

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("--model")
        .arg(&model)
        .arg("--output_file")
        .arg(&tmp)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    // Run from piper's own folder so its bundled espeak-ng-data + onnxruntime.dll resolve.
    if let Some(parent) = Path::new(&bin).parent() {
        cmd.current_dir(parent);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = cmd.spawn().map_err(|e| format!("starting piper: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        drop(stdin); // EOF so piper synthesizes and exits
    }
    let status = child.wait().await.map_err(|e| format!("piper failed: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("piper exited with status {:?}", status.code()));
    }
    let bytes = std::fs::read(&tmp).map_err(|e| format!("reading piper output: {e}"))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(bytes)
}

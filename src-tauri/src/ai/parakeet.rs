//! GPU-accelerated local STT via NVIDIA Parakeet-TDT-0.6b-v3 through parakeet.cpp
//! (mudler/parakeet.cpp). whisper.cpp has no AMD-GPU build; parakeet.cpp ships a
//! Vulkan binary that runs on AMD/Intel/NVIDIA. The prebuilt is CLI-only (no
//! server), so we run `parakeet-cli transcribe … --json` per utterance with
//! CREATE_NO_WINDOW (the spawn pattern that avoids the console-init crash).

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};

use crate::storage::Db;

const MODEL_FILE: &str = "tdt-0.6b-v3-q5_k.gguf";
const MODEL_URL: &str =
    "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/main/tdt-0.6b-v3-q5_k.gguf";

fn parakeet_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"))
        .join("parakeet")
}

fn emit(app: &AppHandle, status: &str, message: &str) {
    let _ = app.emit(
        "voice://parakeet-setup",
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

/// Locate `parakeet-cli` (managed install, configured path, or PATH).
pub fn find_parakeet(app: &AppHandle, override_path: Option<&str>) -> Option<String> {
    let exe = if cfg!(windows) { "parakeet-cli.exe" } else { "parakeet-cli" };
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        if Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    if let Some(found) = find_file(&parakeet_dir(app), exe) {
        return Some(found.to_string_lossy().into_owned());
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

/// Download the prebuilt Parakeet (Vulkan, Windows) + the multilingual model.
/// Returns the binary path.
pub async fn setup_parakeet(app: &AppHandle) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("Auto-install is Windows-only. Install parakeet.cpp and set parakeet.bin_path.".into());
    }
    emit(app, "download", "Finding parakeet.cpp release…");
    let client = reqwest::Client::builder()
        .user_agent("xConsole/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let rel: serde_json::Value = client
        .get("https://api.github.com/repos/mudler/parakeet.cpp/releases/latest")
        .send()
        .await
        .map_err(|e| format!("looking up parakeet.cpp release: {e}"))?
        .json()
        .await
        .map_err(|e| format!("parsing release: {e}"))?;
    let assets = rel.get("assets").and_then(|a| a.as_array()).ok_or("no release assets")?;
    // Vulkan build — runs on AMD/Intel/NVIDIA GPUs.
    let asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.ends_with("bin-win-vulkan-x64.zip"))
                .unwrap_or(false)
        })
        .ok_or("no Windows Vulkan build in the latest parakeet.cpp release")?;
    let url = asset.get("browser_download_url").and_then(|u| u.as_str()).ok_or("asset has no URL")?;

    emit(app, "download", "Downloading Parakeet (GPU)…");
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("reading download: {e}"))?;

    emit(app, "extract", "Extracting…");
    let dir = parakeet_dir(app);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    extract_zip(&bytes, &dir)?;
    let bin = find_file(&dir, "parakeet-cli.exe").ok_or("parakeet-cli.exe not found in archive")?;

    emit(app, "model", "Downloading the Parakeet model (~740 MB)…");
    crate::ai::models::download_gguf(app, "parakeet-model", MODEL_URL, MODEL_FILE).await?;

    emit(app, "done", "GPU speech-to-text ready.");
    Ok(bin.to_string_lossy().into_owned())
}

/// Transcribe a 16 kHz mono WAV with Parakeet on the GPU. `lang` is an ISO code
/// ("ro", "en", …) or "auto".
pub async fn transcribe_parakeet(
    app: &AppHandle,
    db: &Db,
    wav: &[u8],
    lang: &str,
) -> Result<String, String> {
    let bin = find_parakeet(app, db.get_setting("parakeet.bin_path").ok().flatten().as_deref())
        .ok_or("Parakeet not installed. Use Settings → Voice → set up GPU speech-to-text.")?;
    let model = crate::ai::models::models_dir(app).join(MODEL_FILE);
    if !model.exists() {
        return Err(format!("Parakeet model not found: {}", model.display()));
    }
    let tmp = std::env::temp_dir().join(format!("xconsole_pk_{}.wav", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, wav).map_err(|e| format!("writing temp audio: {e}"))?;

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("transcribe")
        .arg("--model")
        .arg(&model)
        .arg("--input")
        .arg(&tmp)
        .arg("--json");
    let lang = lang.trim();
    if !lang.is_empty() && lang != "auto" {
        cmd.arg("--lang").arg(lang);
    }
    if let Some(parent) = Path::new(&bin).parent() {
        cmd.current_dir(parent);
    }
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let out = cmd.output().await;
    let _ = std::fs::remove_file(&tmp);
    let out = out.map_err(|e| format!("parakeet-cli failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "parakeet-cli error: {}",
            String::from_utf8_lossy(&out.stderr).trim().chars().take(160).collect::<String>()
        ));
    }
    // stdout carries the JSON `{"text": "...", ...}` line.
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines().rev() {
        let line = line.trim();
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                return Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string());
            }
        }
    }
    Ok(stdout.trim().to_string())
}

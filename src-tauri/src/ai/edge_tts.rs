//! Free cloud TTS via Microsoft Edge's online neural voices (the `edge-tts` Python
//! package — no API key, high-quality, many languages incl. Romanian). We manage a
//! tiny isolated venv so it never touches the user's global Python, and call it per
//! utterance (text file in → MP3 out). The package handles Microsoft's auth token.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

/// Default Edge voice.
pub const DEFAULT_VOICE: &str = "en-US-AriaNeural";

fn edge_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"))
        .join("edge-tts")
}

fn venv_python(app: &AppHandle) -> PathBuf {
    let venv = edge_dir(app).join("venv");
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

fn find_python() -> Option<String> {
    let finder = if cfg!(windows) { "where" } else { "which" };
    for name in ["python", "py", "python3"] {
        if let Ok(out) = std::process::Command::new(finder).arg(name).output() {
            if out.status.success() {
                if let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next() {
                    let p = line.trim();
                    if !p.is_empty() {
                        return Some(p.to_string());
                    }
                }
            }
        }
    }
    None
}

fn no_window(cmd: &mut tokio::process::Command) {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

/// Create an isolated venv and install `edge-tts` into it. One-time.
pub async fn setup_edge(app: &AppHandle) -> Result<(), String> {
    let py = find_python()
        .ok_or("Python 3 not found. Install Python (python.org) to use the free Edge voice.")?;
    let dir = edge_dir(app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;

    let _ = app.emit("voice://edge-setup", serde_json::json!({"status":"venv","message":"Preparing Python environment…"}));
    let mut venv = tokio::process::Command::new(&py);
    venv.arg("-m").arg("venv").arg("venv").current_dir(&dir);
    no_window(&mut venv);
    let out = venv.output().await.map_err(|e| format!("creating venv: {e}"))?;
    if !out.status.success() {
        return Err(format!("creating venv failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }

    let _ = app.emit("voice://edge-setup", serde_json::json!({"status":"install","message":"Installing edge-tts…"}));
    let vpy = venv_python(app);
    let mut pip = tokio::process::Command::new(&vpy);
    pip.arg("-m").arg("pip").arg("install").arg("--disable-pip-version-check").arg("edge-tts");
    no_window(&mut pip);
    let out = pip.output().await.map_err(|e| format!("pip install: {e}"))?;
    if !out.status.success() {
        return Err(format!("installing edge-tts failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let _ = app.emit("voice://edge-setup", serde_json::json!({"status":"done","message":"Edge voice ready."}));
    Ok(())
}

/// Synthesize `text` to MP3 bytes via Edge TTS.
pub async fn synthesize_edge(app: &AppHandle, text: &str, voice: &str) -> Result<Vec<u8>, String> {
    let vpy = venv_python(app);
    if !vpy.exists() {
        return Err("Edge voice isn't set up yet. Click “Set up Edge voice” in Settings → Voice.".into());
    }
    let voice = if voice.trim().is_empty() { DEFAULT_VOICE } else { voice.trim() };
    let id = uuid::Uuid::new_v4();
    let txt = std::env::temp_dir().join(format!("xconsole_edge_{id}.txt"));
    let mp3 = std::env::temp_dir().join(format!("xconsole_edge_{id}.mp3"));
    std::fs::write(&txt, text).map_err(|e| format!("writing text: {e}"))?;

    let mut cmd = tokio::process::Command::new(&vpy);
    cmd.arg("-m")
        .arg("edge_tts")
        .arg("--voice")
        .arg(voice)
        .arg("--file")
        .arg(&txt)
        .arg("--write-media")
        .arg(&mp3);
    no_window(&mut cmd);
    let out = cmd.output().await;
    let _ = std::fs::remove_file(&txt);
    let out = out.map_err(|e| format!("edge-tts failed: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&mp3);
        return Err(format!("edge-tts error: {}", String::from_utf8_lossy(&out.stderr).trim().chars().take(160).collect::<String>()));
    }
    let bytes = std::fs::read(&mp3).map_err(|e| format!("reading audio: {e}"))?;
    let _ = std::fs::remove_file(&mp3);
    Ok(bytes)
}

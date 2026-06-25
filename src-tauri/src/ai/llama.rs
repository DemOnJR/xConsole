//! Managed local llama.cpp server. xConsole can launch `llama-server` against a
//! downloaded GGUF and health-check it, so a `llamacpp` provider works without the
//! user starting the server by hand. The `llama-server` binary must exist on the
//! machine (on PATH or at a configured path) — it is not bundled.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::process::{Child, Command};

/// Map a build kind to the llama.cpp Windows release asset suffix.
/// "cpu" works anywhere; "vulkan" uses any GPU (incl. AMD); "hip" is AMD ROCm.
fn llama_asset_suffix(build: &str) -> &'static str {
    match build {
        "vulkan" => "bin-win-vulkan-x64.zip",
        "hip" => "bin-win-hip-radeon-x64.zip",
        _ => "bin-win-cpu-x64.zip",
    }
}

/// Download + install a prebuilt `llama-server` (Windows) from the official
/// llama.cpp releases, returning the binary path. `build` selects CPU vs a GPU
/// backend (vulkan/hip) so local models can run on the user's GPU.
pub async fn setup_llama(app: &AppHandle, build: &str) -> Result<String, String> {
    if !cfg!(windows) {
        return Err(
            "Auto-install is Windows-only. Install llama.cpp so `llama-server` is on your PATH, \
             or set its path in Settings (llamacpp.bin_path)."
                .into(),
        );
    }
    let _ = app.emit("llama://setup", serde_json::json!({"status":"download","message":"Finding llama.cpp release…"}));
    let client = reqwest::Client::builder()
        .user_agent("xConsole/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let rel: serde_json::Value = client
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .send()
        .await
        .map_err(|e| format!("looking up llama.cpp release: {e}"))?
        .json()
        .await
        .map_err(|e| format!("parsing release: {e}"))?;
    let assets = rel
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or("no assets in the latest llama.cpp release")?;
    let suffix = llama_asset_suffix(build);
    let asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.ends_with(suffix))
                .unwrap_or(false)
        })
        .ok_or_else(|| format!("no Windows {build} build ({suffix}) in the latest llama.cpp release"))?;
    let url = asset
        .get("browser_download_url")
        .and_then(|u| u.as_str())
        .ok_or("release asset has no download URL")?;

    let _ = app.emit("llama://setup", serde_json::json!({"status":"download","message":"Downloading llama.cpp…"}));
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("reading download: {e}"))?;

    let _ = app.emit("llama://setup", serde_json::json!({"status":"extract","message":"Extracting…"}));
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"))
        .join("llama");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    extract_zip(&bytes, &dir)?;
    let bin = find_file(&dir, "llama-server.exe")
        .ok_or("llama-server.exe not found in the downloaded archive")?;
    let _ = app.emit("llama://setup", serde_json::json!({"status":"done","message":"llama.cpp installed."}));
    Ok(bin.to_string_lossy().into_owned())
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

#[derive(Clone, Default)]
pub struct LlamaServer {
    inner: Arc<Mutex<Option<Running>>>,
}

struct Running {
    child: Child,
    port: u16,
    model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlamaStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub model: Option<String>,
    /// Resolved `llama-server` path, or None if not found.
    pub bin: Option<String>,
}

/// Locate the `llama-server` binary: an explicit override path, else PATH.
pub fn find_binary(override_path: Option<&str>) -> Option<String> {
    if let Some(p) = override_path.map(str::trim).filter(|s| !s.is_empty()) {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["llama-server.exe", "llama-server"]
    } else {
        &["llama-server"]
    };
    let finder = if cfg!(windows) { "where" } else { "which" };
    for n in names {
        if let Ok(out) = crate::proc::quiet_command(finder).arg(n).output() {
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

impl LlamaServer {
    pub fn status(&self, bin_override: Option<&str>) -> LlamaStatus {
        let g = self.inner.lock().unwrap();
        let bin = find_binary(bin_override);
        match g.as_ref() {
            Some(r) => LlamaStatus {
                running: true,
                port: Some(r.port),
                model: Some(r.model.clone()),
                bin,
            },
            None => LlamaStatus {
                running: false,
                port: None,
                model: None,
                bin,
            },
        }
    }

    /// Stop the managed server, if running.
    pub fn stop(&self) {
        if let Some(mut r) = self.inner.lock().unwrap().take() {
            let _ = r.child.start_kill();
        }
    }

    /// Launch `llama-server` for `model_path` on `port`, then wait until /health
    /// responds (or the process exits / times out).
    pub async fn start(
        &self,
        bin: &str,
        model_path: &str,
        port: u16,
        gpu_layers: u32,
    ) -> Result<(), String> {
        if !std::path::Path::new(model_path).exists() {
            return Err(format!("model file not found: {model_path}"));
        }
        self.stop();

        let mut cmd = Command::new(bin);
        cmd.arg("-m")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string());
        if gpu_layers > 0 {
            cmd.arg("-ngl").arg(gpu_layers.to_string());
        }
        // kill_on_drop ties the server's lifetime to ours — it's killed when the
        // managed state drops at app shutdown (or on the next start/stop).
        cmd.kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // CREATE_NO_WINDOW — spawn cleanly from the GUI process (no console flash,
        // and avoids the console-init crash seen when a windowless parent spawns a
        // console child).
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to start llama-server: {e}"))?;
        *self.inner.lock().unwrap() = Some(Running {
            child,
            port,
            model: model_path.to_string(),
        });

        // Health-poll up to ~60s.
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/health");
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // Did the process die during startup?
            let exited = {
                let mut g = self.inner.lock().unwrap();
                match g.as_mut() {
                    Some(r) => r.child.try_wait().ok().flatten().is_some(),
                    None => true, // stopped/replaced elsewhere
                }
            };
            if exited {
                self.stop();
                return Err(
                    "llama-server exited during startup — check the model file and that you have \
                     enough RAM/VRAM (run it manually to see its logs)."
                        .into(),
                );
            }
            if let Ok(resp) = client.get(&url).send().await {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
        }
        self.stop();
        Err("llama-server did not become ready within 60s".into())
    }
}

//! In-app model discovery + download for the local providers (Ollama and
//! llama.cpp / GGUF), plus a system-capability probe so the UI can hide models
//! that won't fit the machine (unless the user asks to "show all").

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::ai::providers::join_url;
use crate::ai::web_tools::http_client;

/// HTTP client for long-running model downloads. Unlike [`http_client`] (a 20s
/// total timeout meant for small fetches), this has NO overall timeout — a
/// multi-GB GGUF or `ollama pull` can run for minutes — just a connect timeout.
fn download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("xConsole/1.0")
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

/// RAM / GPU snapshot used for model-fit filtering (done client-side).
#[derive(Debug, Clone, Serialize)]
pub struct SystemCaps {
    pub ram_mb: u64,
    pub vram_mb: Option<u64>,
    pub gpu_name: Option<String>,
}

/// A model the user can install (an Ollama tag or a Hugging Face GGUF repo/file).
#[derive(Debug, Clone, Serialize)]
pub struct ModelEntry {
    /// Pull name for Ollama (e.g. "llama3.1:8b") or the HF repo id.
    pub id: String,
    pub name: String,
    /// "ollama" | "huggingface"
    pub source: String,
    /// Approx download size in bytes when known (drives the fit badge).
    pub size_bytes: Option<u64>,
    pub detail: String,
    /// True when this Ollama tag is already installed locally.
    pub installed: bool,
}

/// A downloadable GGUF file inside a Hugging Face repo.
#[derive(Debug, Clone, Serialize)]
pub struct HfFile {
    pub file: String,
    pub size_bytes: u64,
    pub url: String,
}

/// Progress event for a running download, emitted on `models://download`.
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub received: u64,
    pub total: Option<u64>,
    pub status: String, // "downloading" | "done" | "error"
    pub message: Option<String>,
}

/// Probe total RAM (sysinfo) and best-effort GPU VRAM (nvidia-smi).
pub fn system_capabilities() -> SystemCaps {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let ram_mb = sys.total_memory() / 1024 / 1024; // sysinfo 0.32 reports bytes

    let (vram_mb, gpu_name) = probe_gpu();
    SystemCaps {
        ram_mb,
        vram_mb,
        gpu_name,
    }
}

fn probe_gpu() -> (Option<u64>, Option<String>) {
    // NVIDIA: nvidia-smi gives exact total VRAM + name (fast path).
    if let Ok(out) = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.total,name",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = s.lines().next() {
                let mut parts = line.split(',');
                let mb = parts.next().and_then(|p| p.trim().parse::<u64>().ok());
                let name = parts.next().map(|p| p.trim().to_string());
                if mb.is_some() {
                    return (mb, name);
                }
            }
        }
    }
    // Any vendor on Windows (AMD/Intel/NVIDIA): read the adapter's dedicated VRAM
    // from the display-driver registry key (accurate for cards >4 GB, unlike WMI).
    #[cfg(windows)]
    {
        if let Some(found) = probe_gpu_windows_registry() {
            return found;
        }
    }
    (None, None)
}

#[cfg(windows)]
fn probe_gpu_windows_registry() -> Option<(Option<u64>, Option<String>)> {
    // Pick the adapter with the largest dedicated VRAM (the discrete GPU).
    let cmd = r#"$d='HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\*'; $a = Get-ItemProperty $d -ErrorAction SilentlyContinue | Where-Object { $_.'HardwareInformation.qwMemorySize' -gt 0 } | Sort-Object { [int64]$_.'HardwareInformation.qwMemorySize' } -Descending | Select-Object -First 1; if ($a) { Write-Output ("{0}|{1}" -f [int64]$a.'HardwareInformation.qwMemorySize', $a.DriverDesc) }"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().find(|l| l.contains('|'))?.trim();
    let (bytes, name) = line.split_once('|')?;
    let mb = bytes.trim().parse::<u64>().ok().map(|b| b / 1024 / 1024);
    let name = name.trim().to_string();
    if mb.unwrap_or(0) == 0 {
        return None;
    }
    Some((mb, if name.is_empty() { None } else { Some(name) }))
}

/// A small curated list of popular Ollama models (the library has no public
/// search API). Sizes are approximate Q4 download sizes. The UI also offers a
/// free-text pull for anything not listed.
pub fn ollama_catalog() -> Vec<ModelEntry> {
    const GB: u64 = 1024 * 1024 * 1024;
    let rows: &[(&str, u64, &str)] = &[
        ("llama3.2:1b", GB, "Meta Llama 3.2 — tiny, fast"),
        ("llama3.2:3b", 2 * GB, "Meta Llama 3.2 — small"),
        ("llama3.1:8b", 5 * GB, "Meta Llama 3.1 — general purpose"),
        ("qwen2.5:7b", 5 * GB, "Qwen2.5 — strong tools/coding"),
        ("qwen2.5:14b", 9 * GB, "Qwen2.5 — larger"),
        ("qwen2.5-coder:7b", 5 * GB, "Qwen2.5 Coder"),
        ("mistral:7b", 4 * GB, "Mistral 7B"),
        ("gemma2:9b", 6 * GB, "Google Gemma 2"),
        ("phi3.5:3.8b", 2 * GB, "Microsoft Phi-3.5 mini"),
        ("deepseek-r1:8b", 5 * GB, "DeepSeek-R1 distill"),
        ("llama3.1:70b", 40 * GB, "Llama 3.1 70B — large"),
        ("qwen2.5:72b", 47 * GB, "Qwen2.5 72B — large"),
    ];
    rows.iter()
        .map(|(id, size, detail)| ModelEntry {
            id: id.to_string(),
            name: id.to_string(),
            source: "ollama".into(),
            size_bytes: Some(*size),
            detail: detail.to_string(),
            installed: false,
        })
        .collect()
}

/// Ollama install / daemon status.
#[derive(Debug, Clone, Serialize)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub bin: Option<String>,
}

/// Locate the `ollama` binary on PATH.
pub fn find_ollama_binary() -> Option<String> {
    let names: &[&str] = if cfg!(windows) {
        &["ollama.exe", "ollama"]
    } else {
        &["ollama"]
    };
    let finder = if cfg!(windows) { "where" } else { "which" };
    for n in names {
        if let Ok(out) = std::process::Command::new(finder).arg(n).output() {
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

/// Whether the Ollama daemon answers at `base_url`.
pub async fn ollama_running(base_url: &str) -> bool {
    let Ok(client) = http_client() else {
        return false;
    };
    client
        .get(join_url(base_url, "api/version"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Ensure the Ollama daemon is running: if it's already up, no-op; if Ollama is
/// installed but stopped, start `ollama serve` and wait for it. Errors (with a
/// download hint) if Ollama isn't installed.
pub async fn ollama_ensure(base_url: &str) -> Result<bool, String> {
    if ollama_running(base_url).await {
        return Ok(true);
    }
    let bin = find_ollama_binary().ok_or(
        "Ollama is not installed. Install it from https://ollama.com/download, then try again.",
    )?;
    // The daemon persists in the background; we don't track the child.
    std::process::Command::new(&bin)
        .arg("serve")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start Ollama: {e}"))?;
    for _ in 0..24 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if ollama_running(base_url).await {
            return Ok(true);
        }
    }
    Err("Ollama was started but didn't become ready in time.".into())
}

/// Locally installed Ollama models via `GET {base}/api/tags`.
pub async fn ollama_list_local(base_url: &str) -> Vec<ModelEntry> {
    let client = match http_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let url = join_url(base_url, "api/tags");
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    body.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m.get("name").and_then(|v| v.as_str())?.to_string();
                    let size = m.get("size").and_then(|v| v.as_u64());
                    Some(ModelEntry {
                        id: name.clone(),
                        name,
                        source: "ollama".into(),
                        size_bytes: size,
                        detail: "installed".into(),
                        installed: true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Recognized publishers shown first: official model authors + the well-known
/// GGUF quantizers the community relies on. Compared case-insensitively against
/// the repo's org (the part before "/").
const TRUSTED_ORGS: &[&str] = &[
    // Official model authors
    "deepseek-ai", "qwen", "meta-llama", "mistralai", "google", "microsoft", "nvidia",
    "ibm-granite", "allenai", "01-ai", "tiiuae", "cohereforai", "stabilityai", "bigcode",
    "openai", "x-ai", "ai21labs", "internlm", "thudm", "baai", "nousresearch",
    // Reputable GGUF repackagers (where GGUF usually actually comes from)
    "ggml-org", "bartowski", "unsloth", "lmstudio-community", "thebloke", "mradermacher",
];

fn org_rank(id: &str) -> u8 {
    let org = id.split('/').next().unwrap_or("").to_lowercase();
    if TRUSTED_ORGS.contains(&org.as_str()) {
        0
    } else {
        1
    }
}

/// Search Hugging Face for GGUF repos matching `query`, ordered with trusted
/// publishers first, then by download count.
pub async fn hf_search(query: &str) -> Result<Vec<ModelEntry>, String> {
    let client = http_client()?;
    let url = format!(
        "https://huggingface.co/api/models?search={}&filter=gguf&limit=40&sort=downloads&direction=-1",
        urlencoding(query)
    );
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HF search failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HF search HTTP {}", resp.status()));
    }
    let arr: serde_json::Value = resp.json().await.map_err(|e| format!("HF parse: {e}"))?;
    let mut rows: Vec<(u8, u64, ModelEntry)> = arr
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let id = m.get("id").and_then(|v| v.as_str())?.to_string();
                    let downloads = m.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                    let trusted = org_rank(&id) == 0;
                    Some((
                        org_rank(&id),
                        downloads,
                        ModelEntry {
                            id: id.clone(),
                            name: id,
                            source: "huggingface".into(),
                            size_bytes: None,
                            detail: if trusted {
                                format!("✓ official · {downloads} downloads")
                            } else {
                                format!("{downloads} downloads")
                            },
                            installed: false,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    // Trusted orgs first (rank 0 before 1), then most-downloaded first.
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    Ok(rows.into_iter().map(|(_, _, e)| e).collect())
}

/// List the GGUF files (with sizes) inside a Hugging Face repo.
pub async fn hf_files(repo_id: &str) -> Result<Vec<HfFile>, String> {
    let client = http_client()?;
    let url = format!("https://huggingface.co/api/models/{repo_id}/tree/main?recursive=true");
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HF files failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HF files HTTP {}", resp.status()));
    }
    let arr: serde_json::Value = resp.json().await.map_err(|e| format!("HF parse: {e}"))?;
    let mut files = Vec::new();
    if let Some(items) = arr.as_array() {
        for it in items {
            let path = it.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.to_lowercase().ends_with(".gguf") {
                let size = it.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                files.push(HfFile {
                    file: path.to_string(),
                    size_bytes: size,
                    url: format!("https://huggingface.co/{repo_id}/resolve/main/{path}"),
                });
            }
        }
    }
    files.sort_by_key(|f| f.size_bytes);
    Ok(files)
}

/// A GGUF file already downloaded into the app models dir.
#[derive(Debug, Clone, Serialize)]
pub struct LocalFile {
    pub file: String,
    pub size_bytes: u64,
}

/// List downloaded GGUF files in the app models dir.
pub fn list_local_gguf(app: &AppHandle) -> Vec<LocalFile> {
    let dir = models_dir(app);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let is_gguf = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false);
            if is_gguf {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                out.push(LocalFile {
                    file: p.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    size_bytes: size,
                });
            }
        }
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}

/// Delete a downloaded GGUF file (basename only — can't escape the models dir).
pub fn delete_local_gguf(app: &AppHandle, filename: &str) -> Result<(), String> {
    let safe = std::path::Path::new(filename)
        .file_name()
        .ok_or("invalid filename")?;
    let path = models_dir(app).join(safe);
    if !path.exists() {
        return Err("file not found".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("delete failed: {e}"))
}

/// Remove an installed Ollama model via `DELETE {base}/api/delete`.
pub async fn delete_ollama(base_url: &str, name: &str) -> Result<(), String> {
    let client = http_client()?;
    let url = join_url(base_url, "api/delete");
    let resp = client
        .delete(url)
        .json(&serde_json::json!({ "name": name }))
        .send()
        .await
        .map_err(|e| format!("ollama delete failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ollama delete HTTP {}", resp.status()));
    }
    Ok(())
}

pub fn models_dir(app: &AppHandle) -> PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("xconsole"));
    base.join("models")
}

/// Download a GGUF file from a (validated public) URL into the app models dir,
/// streaming progress on `models://download`.
pub async fn download_gguf(app: &AppHandle, id: &str, url: &str, filename: &str) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    crate::ai::web_tools::validate_public_url(url)?;
    let client = download_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download HTTP {}", resp.status()));
    }
    let total = resp.content_length();

    let dir = models_dir(app);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("mkdir failed: {e}"))?;
    // Keep only the file's base name to avoid path traversal from the URL.
    let safe = std::path::Path::new(filename)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model.gguf".to_string());
    let dest = dir.join(&safe);
    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| format!("create file failed: {e}"))?;

    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream error: {e}"))?;
        file.write_all(&chunk).await.map_err(|e| format!("write: {e}"))?;
        received += chunk.len() as u64;
        // Throttle progress events to ~every 4 MB.
        if received - last_emit > 4 * 1024 * 1024 {
            last_emit = received;
            let _ = app.emit(
                "models://download",
                DownloadProgress {
                    id: id.to_string(),
                    received,
                    total,
                    status: "downloading".into(),
                    message: None,
                },
            );
        }
    }
    file.flush().await.ok();
    let _ = app.emit(
        "models://download",
        DownloadProgress {
            id: id.to_string(),
            received,
            total,
            status: "done".into(),
            message: Some(dest.to_string_lossy().to_string()),
        },
    );
    Ok(())
}

/// Pull an Ollama model via `POST {base}/api/pull`, streaming progress on
/// `models://download`.
pub async fn ollama_pull(app: &AppHandle, base_url: &str, name: &str) -> Result<(), String> {
    use futures_util::StreamExt;

    let client = download_client()?;
    let url = join_url(base_url, "api/pull");
    let resp = client
        .post(url)
        .json(&serde_json::json!({ "name": name, "stream": true }))
        .send()
        .await
        .map_err(|e| format!("ollama pull failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ollama pull HTTP {}", resp.status()));
    }
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream error: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(nl) = buf.find('\n') {
            let line: String = buf.drain(..=nl).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
                let total = v.get("total").and_then(|s| s.as_u64());
                let completed = v.get("completed").and_then(|s| s.as_u64()).unwrap_or(0);
                let err = v.get("error").and_then(|s| s.as_str());
                let _ = app.emit(
                    "models://download",
                    DownloadProgress {
                        id: name.to_string(),
                        received: completed,
                        total,
                        status: if err.is_some() {
                            "error"
                        } else if status == "success" {
                            "done"
                        } else {
                            "downloading"
                        }
                        .into(),
                        message: err.map(str::to_string).or_else(|| Some(status.to_string())),
                    },
                );
                if let Some(e) = err {
                    return Err(e.to_string());
                }
            }
        }
    }
    Ok(())
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

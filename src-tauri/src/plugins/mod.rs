use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginNavItem {
    pub id: String,
    pub label: String,
    pub icon: String,
    #[serde(default)]
    pub order: Option<i32>,
    #[serde(default)]
    pub badge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAgentTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSettingsSection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCanvasNode {
    pub r#type: String,
    pub title: String,
    #[serde(default)]
    pub default_width: Option<u32>,
    #[serde(default)]
    pub default_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommand {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub nav_item: Option<PluginNavItem>,
    #[serde(default)]
    pub agent_tools: Option<Vec<PluginAgentTool>>,
    #[serde(default)]
    pub settings_section: Option<PluginSettingsSection>,
    #[serde(default)]
    pub canvas_node: Option<PluginCanvasNode>,
    #[serde(default)]
    pub commands: Option<Vec<PluginCommand>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    pub icon: String,
    pub category: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub is_builtin: bool,
    #[serde(default)]
    pub installed_path: Option<String>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
}

fn default_true() -> bool {
    true
}

pub fn get_plugins_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join(".xconsole").join("plugins");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn get_disabled_file_path() -> PathBuf {
    get_plugins_dir().join(".disabled.json")
}

pub fn load_disabled_plugin_ids() -> HashSet<String> {
    let path = get_disabled_file_path();
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(set) = serde_json::from_str::<HashSet<String>>(&content) {
            return set;
        }
    }
    HashSet::new()
}

pub fn save_disabled_plugin_ids(ids: &HashSet<String>) {
    let path = get_disabled_file_path();
    if let Ok(json) = serde_json::to_string_pretty(ids) {
        let _ = fs::write(path, json);
    }
}

/// Discovers both built-in plugins (from local directory) and community installed plugins
pub fn list_plugins() -> Vec<PluginManifest> {
    let mut plugins = Vec::new();
    let disabled_ids = load_disabled_plugin_ids();

    // 1. Scan user plugins directory: ~/.xconsole/plugins/
    let user_plugins_dir = get_plugins_dir();
    if let Ok(entries) = fs::read_dir(&user_plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let manifest_file = path.join("plugin.json");
                if manifest_file.exists() {
                    if let Ok(content) = fs::read_to_string(&manifest_file) {
                        if let Ok(mut manifest) = serde_json::from_str::<PluginManifest>(&content) {
                            manifest.installed_path = Some(path.to_string_lossy().to_string());
                            manifest.is_builtin = false;
                            manifest.enabled = !disabled_ids.contains(&manifest.id);
                            plugins.push(manifest);
                        }
                    }
                }
            }
        }
    }

    // 2. Scan workspace/builtin plugins in `./plugins/` or `../plugins/`
    let candidates = [
        PathBuf::from("plugins"),
        PathBuf::from("../plugins"),
        dirs::home_dir().map(|h| h.join(".xconsole").join("plugins")).unwrap_or_default(),
    ];

    for local_plugins_dir in candidates {
        if local_plugins_dir.exists() {
            if let Ok(entries) = fs::read_dir(&local_plugins_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let manifest_file = path.join("plugin.json");
                        if manifest_file.exists() {
                            if let Ok(content) = fs::read_to_string(&manifest_file) {
                                if let Ok(mut manifest) = serde_json::from_str::<PluginManifest>(&content) {
                                    // Avoid duplicate if already loaded
                                    if !plugins.iter().any(|p| p.id == manifest.id) {
                                        manifest.installed_path = Some(path.to_string_lossy().to_string());
                                        manifest.is_builtin = local_plugins_dir.ends_with("plugins") && !local_plugins_dir.starts_with(dirs::home_dir().unwrap_or_default());
                                        manifest.enabled = !disabled_ids.contains(&manifest.id);
                                        plugins.push(manifest);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    plugins
}

pub fn get_plugin_readme(plugin_id: &str) -> Result<String, String> {
    let plugins = list_plugins();
    if let Some(plugin) = plugins.iter().find(|p| p.id == plugin_id) {
        if let Some(ref path_str) = plugin.installed_path {
            let path = PathBuf::from(path_str).join("README.md");
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    return Ok(content);
                }
            }
        }
    }

    let local_readme = PathBuf::from("plugins").join(plugin_id).join("README.md");
    if local_readme.exists() {
        if let Ok(content) = fs::read_to_string(local_readme) {
            return Ok(content);
        }
    }

    let parent_readme = PathBuf::from("../plugins").join(plugin_id).join("README.md");
    if parent_readme.exists() {
        if let Ok(content) = fs::read_to_string(parent_readme) {
            return Ok(content);
        }
    }

    Ok(format!(
        "# {plugin_id}\n\nDocumentation is available on GitHub: https://github.com/DemOnJR/{plugin_id}"
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInstallProgress {
    pub step: String,
    pub step_index: u32,
    pub total_steps: u32,
    pub percent: u32,
    pub log_line: Option<String>,
    pub is_error: bool,
    pub is_done: bool,
}

async fn run_quiet_tokio_streaming<F>(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    on_log: F,
) -> Result<(), String>
where
    F: Fn(String) + Send + Sync + 'static,
{
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut cmd = if cfg!(windows) && (program == "pnpm" || program == "npm" || program == "npx" || program == "git") {
        let mut c = crate::proc::quiet_tokio("cmd");
        c.arg("/C");
        c.arg(program);
        for a in args {
            c.arg(a);
        }
        c
    } else {
        let mut c = crate::proc::quiet_tokio(program);
        for a in args {
            c.arg(a);
        }
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Eșec la lansarea {program}: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let on_log = std::sync::Arc::new(on_log);

    let stdout_handle = if let Some(stdout) = stdout {
        let on_log_clone = on_log.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if !line.trim().is_empty() {
                    on_log_clone(line);
                }
            }
        })
    } else {
        tokio::spawn(async {})
    };

    let stderr_handle = if let Some(stderr) = stderr {
        let on_log_clone = on_log.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if !line.trim().is_empty() {
                    on_log_clone(line);
                }
            }
        })
    } else {
        tokio::spawn(async {})
    };

    let status = child.wait().await.map_err(|e| format!("Eșec la procesul {program}: {e}"))?;
    let _ = tokio::join!(stdout_handle, stderr_handle);

    if !status.success() {
        return Err(format!("Comanda '{program} {}' a eșuat cu codul {}", args.join(" "), status));
    }

    Ok(())
}

/// 1-Command async installer: streams progress and logs to frontend, suppresses console windows
pub async fn install_plugin_with_progress(
    app: &tauri::AppHandle,
    source: &str,
) -> Result<PluginManifest, String> {
    use tauri::Emitter;

    let source = source.trim();
    if source.is_empty() {
        return Err("Adresa pluginului nu poate fi goală.".into());
    }

    let emit_progress = |step: &str, step_index: u32, total_steps: u32, percent: u32, log_line: Option<String>, is_error: bool, is_done: bool| {
        let payload = PluginInstallProgress {
            step: step.to_string(),
            step_index,
            total_steps,
            percent,
            log_line,
            is_error,
            is_done,
        };
        let _ = app.emit("plugins://install-progress", &payload);
    };

    emit_progress("Inițializare instalare...", 1, 5, 10, Some(format!("Analiză sursă plugin: {source}")), false, false);

    let plugins_dir = get_plugins_dir();
    let is_git_url = source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("git@")
        || (source.contains('/') && !Path::new(source).exists());

    let target_name = if is_git_url {
        let repo_part = source.trim_end_matches(".git").split('/').last().unwrap_or("plugin");
        repo_part.to_string()
    } else {
        Path::new(source)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "plugin".into())
    };

    let target_dir = plugins_dir.join(&target_name);

    if target_dir.exists() {
        emit_progress("Pregătire director...", 1, 5, 15, Some(format!("Curățare versiune anterioară: {}", target_dir.display())), false, false);
        let _ = fs::remove_dir_all(&target_dir);
    }

    if is_git_url {
        let git_url = if !source.starts_with("http") && !source.starts_with("git@") {
            format!("https://github.com/{source}.git")
        } else {
            source.to_string()
        };

        emit_progress(
            "Descărcare repozitoriu Git...",
            2,
            5,
            30,
            Some(format!("git clone --depth 1 {git_url} {}", target_dir.display())),
            false,
            false,
        );

        let app_handle_log = app.clone();
        let on_git_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Descărcare repozitoriu Git...".to_string(),
                step_index: 2,
                total_steps: 5,
                percent: 35,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log.emit("plugins://install-progress", &payload);
        };

        let target_dir_str = target_dir.to_str().unwrap_or("");
        if let Err(e) = run_quiet_tokio_streaming("git", &["clone", "--depth", "1", &git_url, target_dir_str], None, on_git_log).await {
            emit_progress("Eroare la descărcare", 2, 5, 30, Some(format!("Git clone a eșuat: {e}")), true, true);
            return Err(format!("Descărcare eșuată: {e} (Asigură-te că git este instalat și repository-ul este accesibil)"));
        }
    } else {
        let src_path = Path::new(source);
        if !src_path.exists() {
            let err = format!("Calea locală '{source}' nu există.");
            emit_progress("Eroare", 2, 5, 30, Some(err.clone()), true, true);
            return Err(err);
        }
        emit_progress("Copiere fișiere sursă...", 2, 5, 30, Some(format!("Copiere din {} în {}", src_path.display(), target_dir.display())), false, false);
        if let Err(e) = copy_dir_recursive(src_path, &target_dir) {
            let err = format!("Eroare la copiere plugin: {e}");
            emit_progress("Eroare", 2, 5, 30, Some(err.clone()), true, true);
            return Err(err);
        }
    }

    let manifest_path = target_dir.join("plugin.json");
    if !manifest_path.exists() {
        let err = format!("Directorul instalat nu conține un fișier 'plugin.json' valid în {}", target_dir.display());
        emit_progress("Eroare validare", 3, 5, 50, Some(err.clone()), true, true);
        return Err(err);
    }

    // Step 3: Install dependencies
    let pkg_json = target_dir.join("package.json");
    if pkg_json.exists() {
        emit_progress(
            "Instalare dependințe...",
            3,
            5,
            60,
            Some("Se instalează dependințele pluginului (pnpm / npm)...".to_string()),
            false,
            false,
        );

        let app_handle_log = app.clone();
        let on_install_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Instalare dependințe...".to_string(),
                step_index: 3,
                total_steps: 5,
                percent: 65,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log.emit("plugins://install-progress", &payload);
        };

        if let Err(e_pnpm) = run_quiet_tokio_streaming("pnpm", &["install"], Some(&target_dir), on_install_log.clone()).await {
            emit_progress("Instalare dependințe...", 3, 5, 62, Some(format!("pnpm indisponibil ({e_pnpm}), se încearcă npm install...")), false, false);
            if let Err(e_npm) = run_quiet_tokio_streaming("npm", &["install"], Some(&target_dir), on_install_log).await {
                emit_progress("Avertisment instalare dependințe", 3, 5, 70, Some(format!("npm install avertisment: {e_npm}")), false, false);
            }
        }

        // Step 4: Build plugin
        emit_progress(
            "Compilare pachet...",
            4,
            5,
            85,
            Some("Se compilează bundle-ul pluginului (build)...".to_string()),
            false,
            false,
        );

        let app_handle_log2 = app.clone();
        let on_build_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Compilare pachet...".to_string(),
                step_index: 4,
                total_steps: 5,
                percent: 88,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log2.emit("plugins://install-progress", &payload);
        };

        if let Err(e_pnpm) = run_quiet_tokio_streaming("pnpm", &["run", "build"], Some(&target_dir), on_build_log.clone()).await {
            emit_progress("Compilare pachet...", 4, 5, 86, Some(format!("pnpm run build indisponibil ({e_pnpm}), se încearcă npm run build...")), false, false);
            if let Err(e_npm) = run_quiet_tokio_streaming("npm", &["run", "build"], Some(&target_dir), on_build_log).await {
                emit_progress("Avertisment compilare", 4, 5, 90, Some(format!("npm run build avertisment: {e_npm}")), false, false);
            }
        }
    }

    // Step 5: Read & Validate Manifest
    emit_progress("Validare și activare...", 5, 5, 95, Some("Verificare manifest plugin.json...".to_string()), false, false);

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Nu s-a putut citi plugin.json: {e}"))?;
    let mut manifest = serde_json::from_str::<PluginManifest>(&content)
        .map_err(|e| format!("Format invalid în plugin.json: {e}"))?;

    manifest.installed_path = Some(target_dir.to_string_lossy().to_string());
    manifest.enabled = true;
    manifest.is_builtin = false;

    // Un-disable if previously disabled
    let mut disabled = load_disabled_plugin_ids();
    if disabled.remove(&manifest.id) {
        save_disabled_plugin_ids(&disabled);
    }

    emit_progress(
        "Finalizat cu succes!",
        5,
        5,
        100,
        Some(format!("Pluginul '{}' (v{}) a fost instalat și activat cu succes!", manifest.name, manifest.version)),
        false,
        true,
    );

    Ok(manifest)
}

/// 1-Command installer fallback (synchronous, quiet subprocess execution)
pub fn install_plugin(source: &str) -> Result<PluginManifest, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("Plugin source cannot be empty".into());
    }

    let plugins_dir = get_plugins_dir();
    let is_git_url = source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("git@")
        || source.contains('/') && !Path::new(source).exists();

    let target_name = if is_git_url {
        let repo_part = source.trim_end_matches(".git").split('/').last().unwrap_or("plugin");
        repo_part.to_string()
    } else {
        Path::new(source)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "plugin".into())
    };

    let target_dir = plugins_dir.join(&target_name);

    if is_git_url {
        let git_url = if !source.starts_with("http") && !source.starts_with("git@") {
            format!("https://github.com/{source}.git")
        } else {
            source.to_string()
        };

        if target_dir.exists() {
            let _ = fs::remove_dir_all(&target_dir);
        }

        let mut cmd = crate::proc::quiet_command("git");
        cmd.args(["clone", "--depth", "1", &git_url, target_dir.to_str().unwrap_or("")]);
        let output = cmd.output()
            .map_err(|e| format!("Eroare la rularea 'git clone': {e} (Asigură-te că git este instalat)"))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Instalare plugin eșuată: {err}"));
        }
    } else {
        let src_path = Path::new(source);
        if !src_path.exists() {
            return Err(format!("Calea locală '{source}' nu există"));
        }
        if target_dir.exists() {
            let _ = fs::remove_dir_all(&target_dir);
        }
        copy_dir_recursive(src_path, &target_dir)
            .map_err(|e| format!("Eroare la copiere plugin: {e}"))?;
    }

    let manifest_path = target_dir.join("plugin.json");
    if !manifest_path.exists() {
        return Err(format!(
            "Directorul instalat nu conține un fișier 'plugin.json' valid în {}",
            target_dir.display()
        ));
    }

    let _ = build_plugin_locally(&target_dir);

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Nu s-a putut citi plugin.json: {e}"))?;
    let mut manifest = serde_json::from_str::<PluginManifest>(&content)
        .map_err(|e| format!("Format invalid în plugin.json: {e}"))?;

    manifest.installed_path = Some(target_dir.to_string_lossy().to_string());
    manifest.enabled = true;
    manifest.is_builtin = false;

    Ok(manifest)
}

fn run_pm_command(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = crate::proc::quiet_command("cmd");
        c.arg("/C");
        c.arg(args[0]);
        for a in &args[1..] {
            c.arg(a);
        }
        c
    } else {
        let mut c = crate::proc::quiet_command(args[0]);
        for a in &args[1..] {
            c.arg(a);
        }
        c
    };

    cmd.current_dir(dir);
    let output = cmd.output().map_err(|e| format!("Failed to execute '{}': {e}", args.join(" ")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Command '{}' failed: {err}", args.join(" ")));
    }
    Ok(())
}

/// Executes local compilation pipeline (pnpm install && pnpm build)
pub fn build_plugin_locally(plugin_dir: &Path) -> Result<(), String> {
    let pkg_json = plugin_dir.join("package.json");
    if !pkg_json.exists() {
        return Ok(());
    }

    // Step 1: Install dependencies (try pnpm -> npm fallback)
    if let Err(e_pnpm) = run_pm_command(plugin_dir, &["pnpm", "install"]) {
        if let Err(e_npm) = run_pm_command(plugin_dir, &["npm", "install"]) {
            eprintln!("[Plugin Build] Warning during install: {e_pnpm}; {e_npm}");
        }
    }

    // Step 2: Run build script (try pnpm run build -> npm run build fallback)
    if let Err(e_pnpm) = run_pm_command(plugin_dir, &["pnpm", "run", "build"]) {
        if let Err(e_npm) = run_pm_command(plugin_dir, &["npm", "run", "build"]) {
            eprintln!("[Plugin Build] Warning during build: {e_pnpm}; {e_npm}");
        }
    }

    Ok(())
}

/// Links a local plugin repository directory for hot-reload developer workflow
pub fn link_plugin(local_path: &str) -> Result<PluginManifest, String> {
    let src = Path::new(local_path);
    if !src.exists() {
        return Err(format!("Calea locală '{}' nu există", local_path));
    }
    let manifest_path = src.join("plugin.json");
    if !manifest_path.exists() {
        return Err(format!("'{}' nu conține un fișier 'plugin.json'", src.display()));
    }

    // Compile local build before linking
    let _ = build_plugin_locally(src);

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Nu s-a putut citi plugin.json: {e}"))?;
    let mut manifest = serde_json::from_str::<PluginManifest>(&content)
        .map_err(|e| format!("Format invalid în plugin.json: {e}"))?;

    let plugins_dir = get_plugins_dir();
    let target_dir = plugins_dir.join(&manifest.id);

    if target_dir.exists() {
        let _ = fs::remove_dir_all(&target_dir);
    }
    copy_dir_recursive(src, &target_dir)
        .map_err(|e| format!("Eroare la linkarea pluginului: {e}"))?;

    manifest.installed_path = Some(src.to_string_lossy().to_string());
    manifest.enabled = true;
    manifest.is_builtin = false;
    Ok(manifest)
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugins = list_plugins();
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id);

    if let Some(p) = plugin {
        if let Some(path_str) = p.installed_path {
            let path = PathBuf::from(path_str);
            if path.exists() && !p.is_builtin {
                let _ = fs::remove_dir_all(&path);
            }
        }
    }

    let mut disabled = load_disabled_plugin_ids();
    disabled.remove(plugin_id);
    save_disabled_plugin_ids(&disabled);

    Ok(())
}

pub fn toggle_plugin(plugin_id: &str, enabled: bool) -> Result<bool, String> {
    let mut disabled = load_disabled_plugin_ids();
    if enabled {
        disabled.remove(plugin_id);
    } else {
        disabled.insert(plugin_id.to_string());
    }
    save_disabled_plugin_ids(&disabled);
    Ok(enabled)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginUpdateInfo {
    pub plugin_id: String,
    pub plugin_name: String,
    pub current_version: String,
    pub new_version: Option<String>,
    pub current_commit: String,
    pub latest_commit: String,
    pub repository_url: String,
    pub has_update: bool,
    pub is_git_repo: bool,
    pub commit_message: Option<String>,
}

/// Checks if an update is available for a given plugin by querying its Git remote
pub fn check_plugin_update(plugin_id: &str) -> Result<PluginUpdateInfo, String> {
    let plugins = list_plugins();
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Pluginul '{plugin_id}' nu este instalat."))?;

    let installed_path = plugin.installed_path.as_deref().unwrap_or("");
    let plugin_dir = Path::new(installed_path);

    if !plugin_dir.exists() {
        return Err(format!("Directorul pentru pluginul '{plugin_id}' nu a fost găsit."));
    }

    let git_dir = plugin_dir.join(".git");
    let is_git_repo = git_dir.exists();

    let mut repo_url = (plugin.repository.clone())
        .unwrap_or_else(|| format!("https://github.com/DemOnJR/{}", plugin.id));

    let mut current_commit = String::new();
    let mut latest_commit = String::new();
    let mut has_update = false;
    let mut commit_message = None;

    if is_git_repo {
        // 1. Read configured origin URL (handles custom forks and alternative authors)
        let mut get_url_cmd = crate::proc::quiet_command("git");
        get_url_cmd.args(["config", "--get", "remote.origin.url"]).current_dir(plugin_dir);
        if let Ok(out) = get_url_cmd.output() {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    repo_url = u;
                }
            }
        }

        // 2. Read current local HEAD commit
        let mut rev_cmd = crate::proc::quiet_command("git");
        rev_cmd.args(["rev-parse", "HEAD"]).current_dir(plugin_dir);
        if let Ok(out) = rev_cmd.output() {
            if out.status.success() {
                current_commit = String::from_utf8_lossy(&out.stdout).trim().to_string();
            }
        }

        // 3. Query remote HEAD commit without heavy downloads
        let mut ls_cmd = crate::proc::quiet_command("git");
        ls_cmd.args(["ls-remote", "origin", "HEAD"]).current_dir(plugin_dir);
        if let Ok(out) = ls_cmd.output() {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if let Some(hash) = parts.first() {
                        latest_commit = hash.to_string();
                    }
                }
            }
        }

        if !current_commit.is_empty() && !latest_commit.is_empty() && current_commit != latest_commit {
            has_update = true;
            commit_message = Some(format!("Commit nou disponibil: {}", &latest_commit[..latest_commit.len().min(7)]));
        }
    } else if !repo_url.is_empty() {
        // Fallback for non-git directory with a repository URL
        let mut ls_cmd = crate::proc::quiet_command("git");
        ls_cmd.args(["ls-remote", &repo_url, "HEAD"]);
        if let Ok(out) = ls_cmd.output() {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    let parts: Vec<&str> = first_line.split_whitespace().collect();
                    if let Some(hash) = parts.first() {
                        latest_commit = hash.to_string();
                    }
                }
            }
        }
    }

    Ok(PluginUpdateInfo {
        plugin_id: plugin.id,
        plugin_name: plugin.name,
        current_version: plugin.version,
        new_version: None,
        current_commit: if current_commit.len() > 7 { current_commit[..7].to_string() } else { current_commit },
        latest_commit: if latest_commit.len() > 7 { latest_commit[..7].to_string() } else { latest_commit },
        repository_url: repo_url,
        has_update,
        is_git_repo,
        commit_message,
    })
}

/// Scans all installed plugins for updates
pub fn check_all_plugin_updates() -> Vec<PluginUpdateInfo> {
    let plugins = list_plugins();
    let mut results = Vec::new();
    for p in plugins {
        if let Ok(info) = check_plugin_update(&p.id) {
            results.push(info);
        }
    }
    results
}

/// Updates a plugin to its latest remote commit, rebuilding dependencies and assets
pub async fn update_plugin_with_progress(
    app: &tauri::AppHandle,
    plugin_id: &str,
) -> Result<PluginManifest, String> {
    use tauri::Emitter;

    let plugins = list_plugins();
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Pluginul '{plugin_id}' nu este instalat."))?;

    let installed_path = plugin.installed_path.as_deref().unwrap_or("");
    let plugin_dir = Path::new(installed_path);

    if !plugin_dir.exists() {
        return Err(format!("Directorul pluginului '{plugin_id}' nu există."));
    }

    let emit_progress = |step: &str, step_index: u32, total_steps: u32, percent: u32, log_line: Option<String>, is_error: bool, is_done: bool| {
        let payload = PluginInstallProgress {
            step: step.to_string(),
            step_index,
            total_steps,
            percent,
            log_line,
            is_error,
            is_done,
        };
        let _ = app.emit("plugins://install-progress", &payload);
    };

    emit_progress(
        &format!("Inițializare actualizare pentru '{}'...", plugin.name),
        1,
        5,
        10,
        Some(format!("Verificare director: {}", plugin_dir.display())),
        false,
        false,
    );

    let is_git_repo = plugin_dir.join(".git").exists();

    if is_git_repo {
        emit_progress(
            "Descărcare actualizări Git...",
            2,
            5,
            30,
            Some("Rulare git fetch și actualizare la ultimul commit...".to_string()),
            false,
            false,
        );

        let app_handle_log = app.clone();
        let on_git_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Descărcare actualizări Git...".to_string(),
                step_index: 2,
                total_steps: 5,
                percent: 35,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log.emit("plugins://install-progress", &payload);
        };

        // Fetch origin
        if let Err(e) = run_quiet_tokio_streaming("git", &["fetch", "--depth", "1", "origin"], Some(plugin_dir), on_git_log.clone()).await {
            emit_progress("Avertisment fetch", 2, 5, 32, Some(format!("Fetch avertisment: {e}")), false, false);
        }

        // Reset to FETCH_HEAD
        if let Err(e) = run_quiet_tokio_streaming("git", &["reset", "--hard", "FETCH_HEAD"], Some(plugin_dir), on_git_log).await {
            emit_progress("Eroare actualizare", 2, 5, 30, Some(format!("Git reset eșuat: {e}")), true, true);
            return Err(format!("Nu s-au putut prelua modificările prin Git: {e}"));
        }
    } else {
        // If not a git checkout, re-run installation from repository URL
        let repo_url = plugin.repository.as_deref().unwrap_or("");
        if repo_url.is_empty() {
            return Err(format!("Pluginul '{plugin_id}' nu conține un repository valid pentru actualizare."));
        }
        return install_plugin_with_progress(app, repo_url).await;
    }

    // Step 3: Install dependencies
    let pkg_json = plugin_dir.join("package.json");
    if pkg_json.exists() {
        emit_progress(
            "Actualizare dependințe...",
            3,
            5,
            60,
            Some("Verificare și instalare pachete dependente...".to_string()),
            false,
            false,
        );

        let app_handle_log = app.clone();
        let on_install_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Actualizare dependințe...".to_string(),
                step_index: 3,
                total_steps: 5,
                percent: 65,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log.emit("plugins://install-progress", &payload);
        };

        if let Err(e_pnpm) = run_quiet_tokio_streaming("pnpm", &["install"], Some(plugin_dir), on_install_log.clone()).await {
            if let Err(e_npm) = run_quiet_tokio_streaming("npm", &["install"], Some(plugin_dir), on_install_log).await {
                emit_progress("Avertisment npm", 3, 5, 70, Some(format!("npm install avertisment: {e_pnpm}; {e_npm}")), false, false);
            }
        }

        // Step 4: Recompile bundle
        emit_progress(
            "Recompilare modul...",
            4,
            5,
            85,
            Some("Se compilează bundle-ul actualizat...".to_string()),
            false,
            false,
        );

        let app_handle_log2 = app.clone();
        let on_build_log = move |line: String| {
            let payload = PluginInstallProgress {
                step: "Recompilare modul...".to_string(),
                step_index: 4,
                total_steps: 5,
                percent: 88,
                log_line: Some(line),
                is_error: false,
                is_done: false,
            };
            let _ = app_handle_log2.emit("plugins://install-progress", &payload);
        };

        if let Err(e_pnpm) = run_quiet_tokio_streaming("pnpm", &["run", "build"], Some(plugin_dir), on_build_log.clone()).await {
            if let Err(e_npm) = run_quiet_tokio_streaming("npm", &["run", "build"], Some(plugin_dir), on_build_log).await {
                emit_progress("Avertisment build", 4, 5, 90, Some(format!("npm build avertisment: {e_pnpm}; {e_npm}")), false, false);
            }
        }
    }

    // Step 5: Read & Validate Manifest
    emit_progress("Validare și repornire...", 5, 5, 95, Some("Reîncărcare date din plugin.json...".to_string()), false, false);

    let manifest_path = plugin_dir.join("plugin.json");
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Nu s-a putut citi plugin.json: {e}"))?;
    let mut manifest = serde_json::from_str::<PluginManifest>(&content)
        .map_err(|e| format!("Format invalid în plugin.json: {e}"))?;

    manifest.installed_path = Some(plugin_dir.to_string_lossy().to_string());
    manifest.enabled = true;
    manifest.is_builtin = false;

    emit_progress(
        "Actualizat cu succes!",
        5,
        5,
        100,
        Some(format!("Pluginul '{}' (v{}) a fost actualizat cu succes!", manifest.name, manifest.version)),
        false,
        true,
    );

    Ok(manifest)
}

/// Changes the Git remote URL for a plugin (e.g. to switch to a user's fork or custom repo)
pub fn set_plugin_remote_url(plugin_id: &str, new_url: &str) -> Result<String, String> {
    let new_url = new_url.trim();
    if new_url.is_empty() {
        return Err("Adresa remote-ului nu poate fi goală.".into());
    }

    let plugins = list_plugins();
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Pluginul '{plugin_id}' nu este instalat."))?;

    let installed_path = plugin.installed_path.as_deref().unwrap_or("");
    let plugin_dir = Path::new(installed_path);

    if !plugin_dir.exists() {
        return Err(format!("Directorul pluginului '{plugin_id}' nu există."));
    }

    let git_dir = plugin_dir.join(".git");
    if !git_dir.exists() {
        return Err(format!("Pluginul '{plugin_id}' nu este un repozitoriu Git valid."));
    }

    let mut cmd = crate::proc::quiet_command("git");
    cmd.args(["remote", "set-url", "origin", new_url]).current_dir(plugin_dir);
    let out = cmd.output().map_err(|e| format!("Eroare la schimbarea remote-ului: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Schimbarea remote-ului a eșuat: {err}"));
    }

    Ok(new_url.to_string())
}

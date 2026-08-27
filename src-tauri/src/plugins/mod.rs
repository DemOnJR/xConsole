use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

    // 2. Scan workspace/builtin plugins in `./plugins/` relative to current working directory
    let local_plugins_dir = PathBuf::from("plugins");
    if local_plugins_dir.exists() {
        if let Ok(entries) = fs::read_dir(&local_plugins_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_file = path.join("plugin.json");
                    if manifest_file.exists() {
                        if let Ok(content) = fs::read_to_string(&manifest_file) {
                            if let Ok(mut manifest) = serde_json::from_str::<PluginManifest>(&content) {
                                // Avoid duplicate if already loaded from user directory
                                if !plugins.iter().any(|p| p.id == manifest.id) {
                                    manifest.installed_path = Some(path.to_string_lossy().to_string());
                                    manifest.is_builtin = true;
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

    plugins
}

/// 1-Command installer: clones from GitHub repo or copies local path into ~/.xconsole/plugins/
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

        let output = Command::new("git")
            .args(["clone", "--depth", "1", &git_url, target_dir.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("Eroare la rularea 'git clone': {e} (Asigură-te că git este instalat)"))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Instalare plugin eșuată: {err}"));
        }
    } else {
        // Local path copy
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

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Nu s-a putut citi plugin.json: {e}"))?;
    let mut manifest = serde_json::from_str::<PluginManifest>(&content)
        .map_err(|e| format!("Format invalid în plugin.json: {e}"))?;

    manifest.installed_path = Some(target_dir.to_string_lossy().to_string());
    manifest.enabled = true;
    manifest.is_builtin = false;

    Ok(manifest)
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugins = list_plugins();
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Pluginul '{plugin_id}' nu a fost găsit"))?;

    if plugin.is_builtin {
        return Err("Pluginurile implicite (built-in) nu pot fi șterse de pe disc; le poți doar dezactiva.".into());
    }

    if let Some(path_str) = plugin.installed_path {
        let path = PathBuf::from(path_str);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|e| format!("Eroare la ștergerea folderului pluginului: {e}"))?;
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

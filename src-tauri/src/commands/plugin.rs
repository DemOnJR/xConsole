use crate::plugins::{self, PluginManifest, PluginUpdateInfo};
use std::collections::HashSet;

#[tauri::command]
pub fn list_installed_plugins() -> Result<Vec<PluginManifest>, String> {
    Ok(plugins::list_plugins())
}

#[tauri::command]
pub fn get_disabled_plugin_ids_cmd() -> Result<HashSet<String>, String> {
    Ok(plugins::load_disabled_plugin_ids())
}

#[tauri::command]
pub fn get_plugin_readme_cmd(plugin_id: String) -> Result<String, String> {
    plugins::get_plugin_readme(&plugin_id)
}

#[tauri::command]
pub async fn install_plugin_cmd(
    app: tauri::AppHandle,
    source: String,
) -> Result<PluginManifest, String> {
    plugins::install_plugin_with_progress(&app, &source).await
}

#[tauri::command]
pub fn uninstall_plugin_cmd(plugin_id: String) -> Result<(), String> {
    plugins::uninstall_plugin(&plugin_id)
}

#[tauri::command]
pub fn toggle_plugin_cmd(plugin_id: String, enabled: bool) -> Result<bool, String> {
    plugins::toggle_plugin(&plugin_id, enabled)
}

#[tauri::command]
pub fn link_plugin_cmd(path: String) -> Result<PluginManifest, String> {
    plugins::link_plugin(&path)
}

#[tauri::command]
pub fn reload_plugins_cmd() -> Result<Vec<PluginManifest>, String> {
    Ok(plugins::list_plugins())
}

#[tauri::command]
pub fn check_plugin_updates_cmd() -> Result<Vec<PluginUpdateInfo>, String> {
    Ok(plugins::check_all_plugin_updates())
}

#[tauri::command]
pub fn check_single_plugin_update_cmd(plugin_id: String) -> Result<PluginUpdateInfo, String> {
    plugins::check_plugin_update(&plugin_id)
}

#[tauri::command]
pub async fn update_plugin_cmd(
    app: tauri::AppHandle,
    plugin_id: String,
) -> Result<PluginManifest, String> {
    plugins::update_plugin_with_progress(&app, &plugin_id).await
}

#[tauri::command]
pub async fn update_all_plugins_cmd(
    app: tauri::AppHandle,
) -> Result<Vec<PluginManifest>, String> {
    let updates = plugins::check_all_plugin_updates();
    let mut updated_manifests = Vec::new();

    for u in updates {
        if u.has_update {
            if let Ok(manifest) = plugins::update_plugin_with_progress(&app, &u.plugin_id).await {
                updated_manifests.push(manifest);
            }
        }
    }

    Ok(updated_manifests)
}

#[tauri::command]
pub fn set_plugin_remote_cmd(plugin_id: String, remote_url: String) -> Result<String, String> {
    plugins::set_plugin_remote_url(&plugin_id, &remote_url)
}


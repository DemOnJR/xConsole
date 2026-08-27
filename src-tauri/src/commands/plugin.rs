use crate::plugins::{self, PluginManifest};

#[tauri::command]
pub fn list_installed_plugins() -> Result<Vec<PluginManifest>, String> {
    Ok(plugins::list_plugins())
}

#[tauri::command]
pub fn install_plugin_cmd(source: String) -> Result<PluginManifest, String> {
    plugins::install_plugin(&source)
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

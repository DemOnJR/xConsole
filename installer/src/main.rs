// Hide the console window for the GUI installer (release builds only).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod install;

fn main() {
    // When Windows "Apps & Features" runs our UninstallString
    // (`uninstall.exe --uninstall`), do the removal headlessly and exit —
    // never show the installer UI in that case.
    if std::env::args().any(|a| a == "--uninstall") {
        install::run_uninstall();
        return;
    }

    // Headless install (for debugging): run the whole flow with no UI, writing
    // progress only to %LOCALAPPDATA%\xConsole\install.log.
    if std::env::args().any(|a| a == "--install") {
        std::process::exit(install::run_install_headless());
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            install::start_install,
            install::launch_app,
            install::close_installer,
            install::is_update_mode
        ])
        .run(tauri::generate_context!())
        .expect("error while running the xConsole installer");
}

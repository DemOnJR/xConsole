// Self-extracting single-exe launcher for the GNU-built xConsole installer.
//
// The GNU build of the Tauri installer dynamically imports WebView2Loader.dll at LOAD
// time (webview2-com-sys only static-links the loader under MSVC), so that exe cannot
// even start without the DLL beside it. This launcher embeds the installer exe AND the
// loader DLL, unpacks both to a private %TEMP% folder, and runs the installer from
// there (where it finds the DLL). The user ships/handles a SINGLE exe.
//
// It forwards CLI args verbatim (--install / --uninstall / --update / none=GUI) and
// propagates the installer's exit code, so it's a transparent stand-in. It also tells
// the installer (via XC_OUTER_EXE) to register THIS self-contained launcher as the
// %LOCALAPPDATA%\xConsole\uninstall.exe — never the DLL-needing inner exe — so both
// "Apps & Features" uninstall and the in-app `uninstall.exe --update` keep working
// without a loose DLL.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// Embedded at compile time from the installer build output (built first, in ../).
// Path is relative to THIS file: installer/stub/src/ -> ../../target = installer/target.
const INNER_EXE: &[u8] = include_bytes!("../../target/release/xConsole-Setup.exe");
const LOADER_DLL: &[u8] = include_bytes!("../../target/release/WebView2Loader.dll");

/// Drop a diagnostic breadcrumb (there's no stderr in the windows subsystem). Per-pid so
/// concurrent runs don't clobber each other.
fn diag(msg: String) {
    let p = std::env::temp_dir().join(format!("xConsole-Setup-{}-error.txt", std::process::id()));
    let _ = std::fs::write(p, msg);
}

fn main() {
    // A per-process unpack dir so concurrent runs (and a long-lived update window) never
    // clobber each other's files.
    let dir = std::env::temp_dir().join(format!("xConsole-Setup-{}", std::process::id()));
    let exe = dir.join("xConsole-Setup.exe");
    let dll = dir.join("WebView2Loader.dll");

    let unpack = || -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&exe, INNER_EXE)?;
        std::fs::write(&dll, LOADER_DLL)?;
        Ok(())
    };
    if let Err(e) = unpack() {
        diag(format!("could not unpack the installer: {e}"));
        std::process::exit(1);
    }

    // The installer registers a SELF-CONTAINED uninstaller. We point it at THIS launcher
    // via XC_OUTER_EXE. If we can't even locate ourselves we can't produce a valid
    // uninstaller, so abort rather than register the DLL-needing inner from %TEMP%.
    let outer: PathBuf = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            diag(format!("could not locate the launcher: {e}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::process::exit(1);
        }
    };

    // Launch the unpacked installer, forwarding our args. Retry the SPAWN a few times:
    // AV real-time scanning can briefly hold a just-written .exe open, making
    // CreateProcess fail with a sharing/access violation. We only retry a failure to
    // *start* — a normal non-zero exit from the installer is a real result we propagate.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut code = 1;
    let mut spawn_err = None;
    for attempt in 0..4 {
        match Command::new(&exe).args(&args).env("XC_OUTER_EXE", &outer).status() {
            Ok(status) => {
                code = status.code().unwrap_or(1);
                spawn_err = None;
                break;
            }
            Err(e) => {
                spawn_err = Some(e);
                if attempt < 3 {
                    std::thread::sleep(Duration::from_millis(200 * (attempt as u64 + 1)));
                }
            }
        }
    }
    if let Some(e) = spawn_err {
        diag(format!("could not start the installer: {e}"));
    }

    // Clean up the unpack dir. status() blocked until the inner fully exited (the GUI
    // closed, or --uninstall/--update returned after spawning their own detached
    // children), so the inner exe + DLL are unlocked. Best-effort; never affects exit.
    let _ = std::fs::remove_dir_all(&dir);
    std::process::exit(code);
}

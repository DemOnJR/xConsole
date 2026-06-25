//! Spawn helpers that stop this GUI app from flashing a console window on Windows.
//!
//! A release build runs under `windows_subsystem = "windows"` (no console of its own),
//! but every child process it launches WITHOUT the `CREATE_NO_WINDOW` flag pops a brief
//! black CMD box. That's very visible when browsing Settings → Models/Voice, which probe
//! for installed tools (`where`, `nvidia-smi`, `powershell`). Route every subprocess —
//! probes, sidecars, local commands, CLI providers — through these helpers so no console
//! ever appears.

/// Windows flag: create the process with no console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A `std::process::Command` that won't pop a console window on Windows.
pub fn quiet_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A `tokio::process::Command` that won't pop a console window on Windows.
pub fn quiet_tokio(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    hide_console(&mut cmd);
    cmd
}

/// Suppress the console window for an already-built `tokio::process::Command`.
pub fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    let _ = cmd;
}

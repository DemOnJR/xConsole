//! Launch xConsole when the user signs in to Windows.
//!
//! Stored as a per-user Run key (`HKCU\...\Run`), so it needs no admin and
//! uninstall can delete it. The value is the current executable, quoted. If the
//! key is already present we rewrite it on launch, so an in-place update does
//! not keep pointing at a path that no longer exists.

use std::path::Path;

/// Name of the Run value. Matches the product name the installer uses.
pub const VALUE_NAME: &str = "xConsole";

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Command line stored in the Run key: the exe, quoted.
pub fn command_line(exe: &Path) -> Result<String, String> {
    let s = exe.to_str().ok_or("executable path is not valid UTF-8")?;
    if s.contains('"') {
        return Err("executable path contains a quote, cannot register for startup".into());
    }
    Ok(format!("\"{s}\""))
}

#[cfg(windows)]
fn exe_path() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("could not find this executable: {e}"))
}

#[cfg(windows)]
fn read_value() -> Result<Option<String>, String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey(RUN_KEY) {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    match key.get_value::<String, _>(VALUE_NAME) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Whether this user currently has xConsole set to launch at sign-in.
pub fn is_enabled() -> Result<bool, String> {
    #[cfg(windows)]
    {
        Ok(read_value()?.is_some())
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

/// True on Windows, where the Run key exists. Other platforms have no equivalent here.
pub fn is_supported() -> bool {
    cfg!(windows)
}

/// Turn launch-at-sign-in on or off for this user.
pub fn set_enabled(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if on {
            let exe = exe_path()?;
            let cmd = command_line(&exe)?;
            let (key, _) = hkcu
                .create_subkey(RUN_KEY)
                .map_err(|e| format!("could not open the startup key: {e}"))?;
            key.set_value(VALUE_NAME, &cmd)
                .map_err(|e| format!("could not write the startup entry: {e}"))?;
        } else {
            let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
                Ok(k) => k,
                Err(_) => return Ok(()),
            };
            match key.delete_value(VALUE_NAME) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("could not remove the startup entry: {e}")),
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err("Launch at sign-in is only available on Windows.".into())
    }
}

/// If the user already opted in, rewrite the Run value to this build's exe.
///
/// An update replaces the binary in place; without this the key can keep a
/// path from a previous install directory and silently stop launching.
pub fn refresh_if_enabled() {
    if !is_supported() {
        return;
    }
    match is_enabled() {
        Ok(true) => {
            if let Err(e) = set_enabled(true) {
                crate::diag(&format!("autostart refresh failed: {e}"));
            }
        }
        Ok(false) => {}
        Err(e) => crate::diag(&format!("autostart read failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn command_line_quotes_the_path() {
        let p = PathBuf::from(r"C:\Users\bogda\AppData\Local\xConsole\app\xconsole.exe");
        assert_eq!(
            command_line(&p).unwrap(),
            r#""C:\Users\bogda\AppData\Local\xConsole\app\xconsole.exe""#
        );
    }

    #[test]
    fn a_quote_in_the_path_is_refused() {
        let p = PathBuf::from(r#"C:\odd"name\xconsole.exe"#);
        assert!(command_line(&p).is_err());
    }

    #[test]
    fn supported_only_on_windows() {
        assert_eq!(is_supported(), cfg!(windows));
    }
}

fn main() {
    // Embed a COMPLETE Windows application manifest instead of Tauri's minimal default
    // (which declares only the Common-Controls dependency — no trustInfo/asInvoker, no
    // supportedOS, no DPI awareness). A bare, identity-less manifest on an unsigned exe
    // that spawns child processes + opens network/SSH is a classic AV heuristic trigger;
    // a full manifest makes the binary read as ordinary, identifiable desktop software.
    // See src-tauri/app.manifest and installer/ANTIVIRUS.md. Honest hardening — nothing
    // is packed/encrypted/hidden. Goes through tauri-winres' resource compiler (windres
    // on the GNU toolchain users build with), the same path that already embeds the
    // version info, so it is low-risk.
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest")),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

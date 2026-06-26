fn main() {
    // Embed a COMPLETE Windows application manifest instead of Tauri's minimal default
    // (Common-Controls dependency only). A bare, identity-less manifest is part of what
    // makes the unsigned installer score the generic "unsigned bundler" ML verdict; a
    // full manifest (clear identity, asInvoker, supportedOS, DPI) reads as ordinary
    // installer software. See installer/app.manifest and installer/ANTIVIRUS.md.
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest")),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}

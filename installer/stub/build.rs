//! Guard for the single-exe stub. It embeds the installer build output via
//! include_bytes!; fail early with a clear message (instead of a cryptic missing-path
//! error two dirs up) when the installer hasn't been built yet, and re-run when those
//! artifacts change so the embed never goes stale.
use std::path::Path;

fn main() {
    // CARGO_MANIFEST_DIR = installer/stub  ->  parent = installer  ->  target/release.
    let inner = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("stub manifest dir has a parent")
        .join("target")
        .join("release");

    for f in ["xConsole-Setup.exe", "WebView2Loader.dll"] {
        let p = inner.join(f);
        println!("cargo:rerun-if-changed={}", p.display());
        if !p.exists() {
            panic!(
                "\n\n  the single-exe stub embeds the installer build output, which is missing:\n\
                 \x20   {}\n\n  \
                 Build the INSTALLER first, on the GNU toolchain (which emits WebView2Loader.dll):\n\
                 \x20   cd installer && cargo build --release\n  \
                 then build this stub — or just run  installer/build-single-exe.ps1.\n\n  \
                 (Under MSVC the loader is statically linked and no stub is needed: the\n  \
                 installer is already a single self-contained exe.)\n",
                p.display()
            );
        }
    }
}

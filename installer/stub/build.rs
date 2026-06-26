//! Build script for the single-exe stub.
//!
//! Two jobs:
//!  1. Guard the `include_bytes!` embed: fail early with a clear message (instead of a
//!     cryptic missing-path error two dirs up) when the installer hasn't been built yet,
//!     and re-run when those artifacts change so the embed never goes stale.
//!  2. Embed a Win32 resource (VERSIONINFO + application manifest) so the launcher is NOT
//!     a blank-metadata PE. A no-metadata exe that writes and runs an embedded executable
//!     is exactly what AV ML heuristics flag as a dropper (Symantec ML.Attribute, Elastic,
//!     APEX all hit the old stub). Carrying normal company/product/version strings and a
//!     manifest moves those models off "malicious". GNU-only by design: the stub is only
//!     ever built on the GNU toolchain (under MSVC the installer is already a single
//!     self-contained exe and no stub is produced), so we drive `windres` directly — no
//!     extra crates, no SDK headers.
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");

    // 1. Guard the embed inputs.
    // CARGO_MANIFEST_DIR = installer/stub  ->  parent = installer  ->  target/release.
    let inner = Path::new(manifest_dir)
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

    // 2. Compile + link the version/manifest resource (Windows targets only).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    let rc = Path::new(manifest_dir).join("app.rc");
    let manifest = Path::new(manifest_dir).join("app.manifest");
    let obj = Path::new(&out_dir).join("app_res.o");
    println!("cargo:rerun-if-changed={}", rc.display());
    println!("cargo:rerun-if-changed={}", manifest.display());

    // Allow override (e.g. x86_64-w64-mingw32-windres on some setups); default to PATH.
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".to_string());
    // Run from the crate dir so the .rc's relative "app.manifest" reference resolves.
    let status = Command::new(&windres)
        .current_dir(manifest_dir)
        .args(["-i", "app.rc", "-O", "coff", "-o"])
        .arg(&obj)
        .status();

    match status {
        Ok(s) if s.success() => {
            // Hand the COFF object to the linker; its .rsrc section merges into the exe.
            // -bins so it never perturbs build-script linking.
            println!("cargo:rustc-link-arg-bins={}", obj.display());
        }
        Ok(s) => println!(
            "cargo:warning=windres exited with {s}; the stub will build WITHOUT version \
             metadata (it will keep working, but AV heuristics may flag it). Ensure MinGW's \
             windres is on PATH."
        ),
        Err(e) => println!(
            "cargo:warning=could not run windres ({e}); the stub will build WITHOUT version \
             metadata. Ensure MinGW's windres is on PATH."
        ),
    }
}

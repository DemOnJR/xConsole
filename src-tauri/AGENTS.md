# Building & releasing xConsole — read this before you compile

This is a **Tauri 2** app: Rust backend here in `src-tauri/`, React/TS frontend in `../src`.
If the user asks you to **compile / build / make an installer / cut a release**, follow this.

## Build & check commands

Run these from the **project root** (`..`), not from `src-tauri/`:

- **Dev (hot reload):** `pnpm tauri dev`
- **Production build** (compiles the app binary — xConsole is distributed via the
  clone+compile installer, **not** a Tauri bundle): on this toolchain,
  `cargo +stable-x86_64-pc-windows-gnu build --release` from `src-tauri/`.
  - Output: `src-tauri/target/release/xconsole.exe` (+ `WebView2Loader.dll`)
- **Frontend typecheck:** `npx tsc --noEmit`

Run this from **`src-tauri/`**:

- **Rust compile check (authoritative & fast):** `cargo build`
  - Prefer this to verify backend changes. The `pnpm tauri dev` watcher can crash on
    Rust hot-reload (exit `0xC0000142` / `3221225794`); `cargo build` is the source of truth.
  - Note: `cargo test` may fail to *launch* the test binary in some Windows shells
    (`STATUS_ENTRYPOINT_NOT_FOUND`, a native-DLL link quirk) — that's environmental, not a
    code failure. Treat `cargo build` success as the gate.

## Cutting a release (clone + compile distribution)

xConsole is **not** shipped as a signed Tauri bundle. It's distributed via the
clone+compile installer in `../installer/`, and the in-app updater
(`src/commands/update.rs`) rebuilds from `origin/main`. So there's no version tag/bump and
no signing step — releases are commit-based off `main`:

1. Merge your changes to `main`.
2. The **Build installer** workflow (`../.github/workflows/installer-release.yml`) builds
   `xConsole-Setup.exe` on `windows-latest` and publishes it to the rolling
   `installer-latest` GitHub Release (marked `--latest`), so `…/releases/latest` always
   serves the newest installer. No secrets needed (preinstalled `rustup` + `gh` +
   automatic `GITHUB_TOKEN`).

Existing users get an "Update available" prompt (their checkout is behind `origin/main`)
and rebuild in one click. See `../RELEASING.md` for the full picture.

## Data safety — never break this

All user data lives in the **OS app-data dir** (`%APPDATA%\com.xconsole.app`: the SQLite DB =
chats/workspaces/settings/providers, plus the agent home) and the **OS keychain** (API keys,
SSH keys) — **never** in the repo or the install directory. An update only replaces the binary.

- Keep DB schema changes **additive**: `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE`. Never drop
  or recreate user tables.
- The app already snapshots `xconsole.db` → `xconsole.db.bak` before a new version's first run
  (see `src/lib.rs` setup) as a safety net.
- Never write user data into the repo or the install dir.

## Key files in this directory

- `Cargo.toml` — Rust dependencies.
- `tauri.conf.json` — app/window config. `bundle.active` is `false` (xConsole ships via the
  clone+compile installer, not a Tauri bundle).
- `capabilities/default.json` — frontend permissions for the main window.
- `src/lib.rs` — app setup: DB open, plugin registration, pre-update DB backup, command registry.
- `src/mcp/server.rs` — the stdio MCP server Cursor uses (run/read/write/canvas/brief tools).

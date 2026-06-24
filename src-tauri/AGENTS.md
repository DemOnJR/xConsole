# Building & releasing xConsole — read this before you compile

This is a **Tauri 2** app: Rust backend here in `src-tauri/`, React/TS frontend in `../src`.
If the user asks you to **compile / build / make an installer / cut a release**, follow this.

## Build & check commands

Run these from the **project root** (`..`), not from `src-tauri/`:

- **Dev (hot reload):** `pnpm tauri dev`
- **Production build** (Windows NSIS installer + signed update artifacts): `pnpm tauri build`
  - Output: `src-tauri/target/release/bundle/nsis/xConsole_<version>_x64-setup.exe` (and its `.sig`)
- **Frontend typecheck:** `npx tsc --noEmit`

Run this from **`src-tauri/`**:

- **Rust compile check (authoritative & fast):** `cargo build`
  - Prefer this to verify backend changes. The `pnpm tauri dev` watcher can crash on
    Rust hot-reload (exit `0xC0000142` / `3221225794`); `cargo build` is the source of truth.
  - Note: `cargo test` may fail to *launch* the test binary in some Windows shells
    (`STATUS_ENTRYPOINT_NOT_FOUND`, a native-DLL link quirk) — that's environmental, not a
    code failure. Treat `cargo build` success as the gate.

## Signing is REQUIRED for release builds (auto-update will reject unsigned bundles)

`pnpm tauri build` only signs the updater artifacts when these env vars are set:

- `TAURI_SIGNING_PRIVATE_KEY` — the updater private key (string contents OR a path).
  The key file is `../.tauri-signing/xconsole.key` (**gitignored — never commit or print it**).
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — **empty** (the key has no password).

PowerShell example for a local signed build:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw ..\.tauri-signing\xconsole.key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
pnpm tauri build
```

Without these, you still get an installer but **no valid `.sig`**, so existing installs will
refuse to auto-update. For real releases, prefer the CI path below (it signs from repo secrets).

## Cutting a release (the normal path — CI builds & signs)

1. Bump `"version"` in **both** `../package.json` and `tauri.conf.json` to the new number.
2. `git commit -am "Release vX.Y.Z" && git tag vX.Y.Z && git push origin main --tags`
3. The GitHub Actions **Release** workflow (`../.github/workflows/release.yml`) builds + signs +
   creates a **draft** GitHub release with the installer + `latest.json`. Review it, then
   **Publish** — only a published (non-draft, non-prerelease) release reaches users via
   auto-update (`endpoints` → `…/releases/latest/download/latest.json`).

The detailed release runbook + the exact GitHub secrets to configure are kept in a
private local note (not committed to this repo).

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

- `Cargo.toml` — Rust dependencies (incl. `tauri-plugin-updater`, `tauri-plugin-process`).
- `tauri.conf.json` — bundle + updater config (`endpoints`, `pubkey`, NSIS `installMode`).
  The `pubkey` here is the **public** half of the signing key; it is safe to commit.
- `capabilities/default.json` — frontend permissions (incl. `updater:default`, `process:default`).
- `src/lib.rs` — app setup: DB open, plugin registration, pre-update DB backup, command registry.
- `src/mcp/server.rs` — the stdio MCP server Cursor uses (run/read/write/canvas/brief tools).

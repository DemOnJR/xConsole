# xConsole Developer & Architecture Rules

## 1. Iconography Rules
- **STRICTLY NO EMOJIS**: Never use emojis anywhere in UI, components, buttons, tabs, tree nodes, or labels.
- **Pure SVG Icons**: All icons across core and plugins must be clean, crisp SVG components imported from `src/components/icons.tsx` (or the plugin's local `icons.tsx`).
- **Muted, Professional Aesthetics**: Avoid noisy or multicolored clutter. Use refined, modern dark mode styling with subtle accents.

## 2. Microkernel & Plugin Architecture
- `xConsole` is a minimal microkernel host.
- Core features (`sftp`, `database`, `agent`, `cloudflare`) live under `plugins/` and are
  mirrored to dedicated GitHub repos (`xconsole-plugin-*`).
- **Note:** `.gitmodules` lists these as submodules, but none is initialised — the plugin
  files are tracked directly in this repo. Edit them in place and commit here; do not
  `cd` into one expecting a separate repository, because git will walk up and you will
  be committing to the parent without noticing.
- Core communicates with plugins via `@xconsole/sdk` (`src/sdk/index.ts`) and dynamic React Flow nodes (`DynamicPluginNode`).

## 3. The WhatsApp Sidecar
- Remote control over WhatsApp needs `src-tauri/sidecar/whatsapp` — a small Go binary
  built on `whatsmeow`, because pairing by QR means the multi-device protocol and no
  Rust crate speaks it.
- **Build it before packaging**: `src-tauri/sidecar/whatsapp/build.sh` (honours `GOOS`
  / `GOARCH`). The installer copies the result beside the xConsole executable; in a
  development tree it is found where the script leaves it.
- The binary is **not** committed — it is 25MB and platform-specific. The Go sources
  are. A build without it still ships Discord and Telegram, and the settings screen
  says WhatsApp is unavailable rather than hanging on a QR that never arrives.
- `CGO_ENABLED=0` is deliberate (pure-Go SQLite), so every target cross-compiles from
  any host with only the Go toolchain.
- The sidecar makes no authorisation decisions. It reports who said what; the
  allowlist lives in `src-tauri/src/ai/remote/mod.rs`. Keep it that way.

## 4. Git Workflow Rules
- **First Pull, Then Edit, Then Push**:
  - Always run `git pull` before making any edits in the core repo or any plugin repository (`plugins/*`).
  - Verify changes with appropriate builds and tests (`pnpm run build` / `tsc` / `vitest`).
  - Commit and `git push` all verified changes back to their respective remote repositories.


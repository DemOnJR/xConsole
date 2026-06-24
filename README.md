# xConsole

A multi-VPS canvas terminal. Drop live SSH terminals for all your servers onto a
single zoomable, pannable canvas, switch between them instantly, and broadcast
commands to many at once.

Built with **Tauri 2** (Rust) + **React 19** + **xterm.js** + **React Flow**, with a
pure-Rust SSH stack (**russh**).

## Features

- **Canvas of terminals** - each VPS is a draggable, resizable xterm.js node on an
  infinite canvas with zoom, pan, and a minimap.
- **VPS picker** - add servers from a searchable sidebar; click to drop a connected
  terminal on the canvas.
- **Layout modes** - Freeform, Snap-to-grid, and one-click Tile auto-arrange.
- **Workspaces** - save/restore named layouts (which servers, positions, viewport).
- **Broadcast** - type one command and send it to all selected terminals.
- **Remote code editing** - right-click a file in the SFTP browser and pick *Edit* to
  open it in an in-app editor; save writes it straight back over SFTP (`Ctrl/⌘+S`).
- **Focus mode** - double-click a terminal header to zoom into it; `Ctrl+Tab` cycles.
- **Tiered rendering** - WebGL is reserved for focused terminals (LRU-capped) so the
  app stays under the webview's ~16 WebGL-context limit when many VPS are open.

## Security

See [SECURITY.md](SECURITY.md). Highlights:

- SSH keys are referenced by **path** only; passwords/passphrases live in the **OS
  keychain** (never in the local database) and key material is **zeroized** after use.
- **Host key verification** (trust-on-first-use) protects against MITM.
- **7-day dependency cooldown** on both npm (pnpm) and Cargo dependencies to defend
  against supply-chain attacks.

## Prerequisites (Windows)

- Node.js 22+ and **pnpm** (`corepack enable pnpm`)
- Rust via rustup. This project links with the **GNU** toolchain plus MinGW gcc
  (no Visual Studio required):

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
winget install -e --id BrechtSanders.WinLibs.POSIX.UCRT --scope user
# In src-tauri:
rustup override set stable-x86_64-pc-windows-gnu
```

Ensure the MinGW `bin` directory and `~/.cargo/bin` are on your PATH.

## Develop

```powershell
pnpm install           # respects the 7-day dependency cooldown
pnpm tauri dev         # launches the desktop app with hot reload
```

## Build

```powershell
pnpm tauri build
```

## Project layout

```
src/                     React UI
  components/            Sidebar, CanvasFlow, TerminalNode, Toolbar, VpsForm
  stores/                Zustand: vps, canvas, session, workspace
  lib/tauri.ts           Typed IPC + event bridge
src-tauri/src/
  ssh/                   russh client, session manager, host-key TOFU
  storage/               SQLite (VPS, workspaces, known_hosts)
  secrets.rs             OS keychain (keyring) + zeroize
  commands/              Tauri command handlers
```

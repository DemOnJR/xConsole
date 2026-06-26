<div align="center">

# xConsole

**All your servers, on one canvas.**

xConsole is a desktop app that puts a live SSH terminal for every one of your
servers onto a single zoomable, pannable canvas — switch between them instantly,
broadcast a command to many at once, and let a built-in AI assistant help you run
and manage them.

Built with **Tauri 2** (Rust) · **React 19** · **xterm.js** · **React Flow**, on a
pure-Rust SSH stack (**russh**).

</div>

---

> ## 🔒 Important: xConsole is a *local* app — don't run it on a public server
>
> xConsole is meant to run **on your own computer**, in **local mode only**. **Do not
> deploy it on a public / internet-facing server or expose it on a public IP address.**
>
> Why: xConsole holds the keys to your infrastructure — your **SSH credentials** and
> **cloud/API keys** (kept in your operating system's keychain) — and its built-in AI
> assistant can **run shell commands on your machine and on your servers**. It is
> designed for a single, trusted machine that only you control.
>
> It never opens a port to the outside world: its only background services (the local
> AI helpers) listen on `127.0.0.1` (localhost) and are unreachable from the network.
> So running it on a public box gains you nothing and risks exposing your credentials.
> **Keep it on your desktop. 👍**

---

## ✨ What it does

- **A canvas of terminals** — each server is a draggable, resizable terminal on an
  infinite canvas with zoom, pan, and a minimap.
- **Quick connect** — add servers from a searchable sidebar; click one to drop a
  connected terminal onto the canvas.
- **Broadcast** — type one command and send it to every selected terminal at once.
- **Workspaces** — save and restore named layouts (which servers, where, and the view).
- **Layout modes** — Freeform, Snap-to-grid, and one-click Tile auto-arrange.
- **Remote file editing** — right-click a file in the SFTP browser → *Edit* to open it
  in an in-app editor; `Ctrl/⌘+S` saves it straight back over SFTP.
- **Built-in AI assistant** — ask it to check, fix, or set things up across your
  servers. Works with local models (Ollama / llama.cpp) or cloud providers.
- **Focus mode** — double-click a terminal header to zoom in; `Ctrl+Tab` cycles.

## 🚀 Install (the easy way)

xConsole compiles itself from source on your PC, so you always run a build you can
inspect — and there's a one-click installer that handles everything for you.

1. **[➡️ Download the installer](https://github.com/DemOnJR/xConsole/releases/latest)**
   — grab `xConsole-Setup-windows-x64.zip` from the latest release.
2. **Right-click the zip → Extract All.**
3. **Double-click `xConsole-Setup.exe`** and press **Install**.

That's it. The installer downloads the build tools it needs and compiles xConsole on
your machine (about **10–20 minutes** the first time). It needs an **internet
connection**, but **no administrator rights** — everything installs neatly under your
own user folder.

> **Updating later** is just as easy: xConsole tells you when there's a new version and
> updates itself in one click. Your chats, workspaces, settings, and saved keys are
> always kept safe across updates.

*(Windows is supported today. macOS/Linux builds are on the roadmap.)*

> ### 🛡️ If your antivirus warns about it
>
> Because xConsole **compiles a fresh build on your own PC**, the resulting `xconsole.exe`
> is **unsigned and unique to your machine** — so it has no "reputation" yet with
> SmartScreen or antivirus engines like Kaspersky. Some may show a warning, or (in
> Kaspersky's case) quarantine the running app. **This is a false positive, not malware** —
> the app is fully open source and scans statically clean on VirusTotal (0 detections,
> Kaspersky's own engine included). It's flagged purely for being a brand-new unsigned
> binary that opens SSH/network connections.
>
> If it happens, you can safely allow it:
> - **Windows SmartScreen:** click **More info → Run anyway**.
> - **Kaspersky:** *Settings → Threats and Exclusions → Manage exclusions* → add the folder
>   `%LOCALAPPDATA%\xConsole` (and add `xconsole.exe` under *trusted applications*). Helping
>   everyone: report it as a false alarm at <https://opentip.kaspersky.com/>.
>
> Technical details and the full remedy (code signing) are in
> **[installer/ANTIVIRUS.md](installer/ANTIVIRUS.md)**.

## 🛠️ Build from source (for developers)

Prefer to build it yourself? You'll need:

- **Node.js 22+** and **pnpm** (`corepack enable pnpm`)
- **Rust** via rustup. xConsole links with the **GNU** toolchain + MinGW gcc, so you
  *don't* need Visual Studio:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
winget install -e --id BrechtSanders.WinLibs.POSIX.UCRT --scope user
# In the src-tauri folder:
rustup override set stable-x86_64-pc-windows-gnu
```

Make sure the MinGW `bin` directory and `~/.cargo/bin` are on your `PATH`. Then:

```powershell
pnpm install      # respects a 7-day dependency cooldown (see Security)
pnpm tauri dev    # run with hot reload
pnpm tauri build  # produce a release build
```

## 🛡️ Is it safe?

Yes — and because xConsole is **open source**, you don't have to take our word for it:
the security doesn't rely on hiding the code. Highlights:

- **Your secrets stay in the OS keychain.** SSH passwords/passphrases and API keys live
  in Windows Credential Manager (or the macOS/Linux equivalent) — **never** in the app's
  database — and key material is wiped from memory right after it's used.
- **Optional app lock (encryption at rest).** Turn on a master password and your local
  database is encrypted with **AES-256-GCM**; the key is derived with **PBKDF2** and can
  be remembered on your trusted device via the OS keychain. There's no backdoor and no
  recovery — a stolen database file is just noise without your password.
- **Host-key verification (TOFU)** protects your SSH sessions against man-in-the-middle.
- **Supply-chain hardening** — a **7-day cooldown** on every dependency (npm *and* Cargo)
  plus advisory/license audits in CI, so a freshly-published malicious package can't
  sneak in.
- **Local-only by design** (see the note at the top).

Full details are in **[SECURITY.md](SECURITY.md)**. Found a vulnerability? Please report
it privately to the maintainers before disclosing it publicly.

## 🧱 Project layout

```
src/                       React UI
  components/              Sidebar, CanvasFlow, TerminalNode, Toolbar, AgentPanel…
  stores/                 Zustand state: vps, canvas, session, workspace, agent, lock
  lib/tauri.ts            Typed IPC + event bridge
src-tauri/src/
  ssh/                    russh client, session manager, host-key TOFU
  ai/                     the built-in agent (context, tools, providers, voice)
  storage/                SQLite + at-rest encryption
  crypto.rs / lock.rs     master-password key wrapping + the lock manifest
  secrets.rs              OS keychain (keyring) + zeroize
  mcp/                    local MCP server (stdio)
  commands/               Tauri command handlers
installer/                the one-click "clone + compile on your PC" setup app
```

## 📄 License

xConsole is released under the **[MIT License](LICENSE)**.

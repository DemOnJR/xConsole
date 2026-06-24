# Releasing xConsole (installer + auto-update)

xConsole ships a Windows installer via **GitHub Releases** and auto-updates itself.
Updates are **cryptographically signed** and only ever replace the app binary —
**user data is never touched** (see "Why your data is safe" below).

## One-time setup

1. **Create the GitHub repo** and push the code. The updater is wired to
   `https://github.com/DemOnJR/xConsole`. If your repo name isn't `xConsole`,
   change it in **one** place: the `endpoints` URL in
   [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json).

2. **Add the signing secrets** (repo → Settings → Secrets and variables → Actions
   → New repository secret):
   - `TAURI_SIGNING_PRIVATE_KEY` — the full contents of `.tauri-signing/xconsole.key`
     (this file is gitignored; open it and paste the whole thing).
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — leave **empty** (the key was generated
     without a password).

   > Keep `.tauri-signing/xconsole.key` somewhere safe (a password manager). If you
   > lose it you can't publish updates the existing installs will accept — you'd have
   > to ship a new public key, which old versions won't trust.

## Cutting a release

1. Bump the version in **both** files to the new number (e.g. `0.1.1`):
   - `package.json` → `"version"`
   - `src-tauri/tauri.conf.json` → `"version"`
2. Commit, then tag and push:
   ```bash
   git commit -am "Release v0.1.1"
   git tag v0.1.1
   git push origin main --tags
   ```
3. The **Release** workflow builds the signed installer + updater manifest and
   creates a **draft** GitHub release with `xConsole_0.1.1_x64-setup.exe`,
   its `.sig`, and `latest.json` attached.
4. Open the draft release on GitHub, sanity-check it, and click **Publish**.
   - New users: download the `…-setup.exe` from the release page.
   - Existing users: get an "Update available" prompt on next launch (or via
     **Settings → General → Check for updates**) and update in one click.

> The auto-updater reads `…/releases/latest/download/latest.json`, and GitHub's
> "latest" ignores **drafts** and **pre-releases** — so nothing reaches users until
> you click Publish on a normal (non-prerelease) release.

## Why your data is safe across updates

An update **only replaces the app executable**. Everything the user cares about
lives elsewhere and is never deleted:

| Data | Location | Touched by an update? |
| --- | --- | --- |
| Chats, workspaces, settings, providers, cron | SQLite `xconsole.db` in `%APPDATA%\com.xconsole.app` | **No** |
| Agent memory / soul / skills / project briefs | `%APPDATA%\com.xconsole.app\agent` | **No** |
| Downloaded models + voice tools | `%APPDATA%\com.xconsole.app\{models,whisper,piper,llama,...}` | **No** |
| API keys, SSH keys, passwords | Windows Credential Manager (OS keychain) | **No** |
| The app binary | `%LOCALAPPDATA%\xConsole` | Replaced |

This holds because:
- All app data derives from Tauri's `app_data_dir()` (`%APPDATA%`) or the OS
  keychain — **never** the install directory.
- The NSIS installer runs in `currentUser` mode and installs to `%LOCALAPPDATA%`,
  a different location from the data.
- On update, the installer runs with `/UPDATE`, which Tauri documents as preserving
  app data (and `deleteAppDataOnUninstall` is **off** — and only ever applies to a
  full uninstall, not an update).
- **Extra safety net:** on the first launch of a *new* version, the app snapshots
  the database to `%APPDATA%\com.xconsole.app\xconsole.db.bak` **before** running any
  schema migration. So even a buggy future migration can't lose data — you can
  restore the `.bak`. (Keep future schema changes additive — `CREATE TABLE IF NOT
  EXISTS` / `ALTER TABLE` — and chats/workspaces/settings carry over cleanly.)

### Verify it yourself (recommended once)
1. Install a build, then use the app: add a workspace, chat with the agent, set an
   API key.
2. Bump the version, cut a release, and let the app auto-update (or run the new
   installer over the old one).
3. Confirm your workspace, chat history, settings, and saved keys are all still
   there. They will be.

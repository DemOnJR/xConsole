# Antivirus false positives — diagnosis & remedy

xConsole and its installer are **unsigned, freshly-built native executables that spawn
child processes and make network/SSH connections**. That profile is the textbook trigger
for *heuristic / machine-learning* antivirus false positives — not because anything is
malicious, but because reputation-based engines treat "unknown publisher + installer/agent
behavior" as suspicious until a binary earns reputation.

This document records what was measured, what was fixed in the code, and the one change
that durably removes the remaining flags: **code signing**.

## What was measured (VirusTotal, 75 engines — re-scanned 2026-06-26)

| Binary | Score range across rebuilds (of 75) | What flags it (when it does) |
| --- | --- | --- |
| **Main app** `xconsole.exe` | **0–1 / 75** | `Elastic` (moderate confidence), intermittently |
| **Installer** `xConsole-Setup.exe` | **1–2 / 75** | `Microsoft: Trojan:Win32/Wacatac.B/C!ml`, `Bkav` |
| **Stub** (single-exe launcher) | **3–4 / 75** | `Symantec ML.Attribute.HighConfidence`, `Elastic`, `APEX`, `Bkav` |

Every flag is an `!ml` / `ML.Attribute` / "high/moderate confidence" **machine-learning**
verdict — none is a known-malware-family signature match. They are reputation/heuristic
verdicts for "unsigned thing that statistically resembles installers/droppers."

**These scores are not stable across rebuilds — and that is the whole point.** Rebuilding the
*identical* source produces a new file hash, and unsigned binaries are re-judged from scratch,
so individual ML engines flip on and off between builds. In one measured rebuild cycle the
main app went 0→1 (Elastic appeared), the installer went 1→2 (Bkav appeared), and the stub
went 4→3 (Bkav *dropped*) — verdicts wandering in **both** directions at once. A no-manifest
control rebuild of the main app was scanned to confirm the wander is the rebuild, not the
manifest. This instability is exactly what a stable, signed identity eliminates: signing lets
reputation accrue to *your publisher* instead of resetting every build. The main app is
statically clean (Kaspersky's own VT engine never flags it); the rare Elastic hit is a
moderate-confidence ML false positive on an unsigned 48 MB binary.

> Reproduce these numbers anytime: the binaries can be uploaded to VirusTotal via its API
> (≤32 MB direct; the 48 MB main app uses the `/files/upload_url` large-file endpoint).
> A reusable scan-and-poll script pattern lives in the project's scratch notes.

Key takeaways:

1. **The main app is not detected by any static engine, including Kaspersky's.** When
   Kaspersky closed the running app, that was its **behavioral engine (System Watcher /
   HIPS) or cloud reputation (KSN)** reacting to an *unsigned* binary — one with a brand
   new hash every rebuild, hence zero reputation — launching a hidden child process (the
   Cursor agent's `node.exe`) that then opens network/SSH connections. There is no
   malicious code to remove; the binary is clean. **Signing is the fix.**

2. The detections are all `!ml` / `ML.Attribute` / `high confidence` **machine-learning**
   verdicts. None is a real signature match for a known malware family. These are the
   verdict names AV vendors emit for "unsigned thing that statistically resembles
   installers/droppers we've seen."

3. `Program:Win32/Wacapew.C!ml` is Microsoft's generic label for **unsigned software
   bundlers/installers**. It is one of the most common false positives for unsigned NSIS,
   Inno, and Tauri installers in existence. It is driven by *unsigned + installer
   behavior*, not by file metadata.

## What was fixed in the code

These reduce the surface and are the right thing to do regardless of signing:

- **The main app and installer now embed a *complete* Windows application manifest**
  (added 2026-06-26). Tauri's built-in default manifest declares only the Common-Controls
  dependency — **no `trustInfo`/`asInvoker`, no `supportedOS`, no DPI awareness** — so an
  unsigned exe that spawns child processes and opens network/SSH had a bare, identity-less
  manifest, part of the suspicious profile. `src-tauri/app.manifest` and
  `installer/app.manifest` now declare a clear app identity (`xConsole.Application` /
  `xConsole.Setup`), `asInvoker` (never `requireAdministrator`), the four Win7–11
  `supportedOS` GUIDs, `PerMonitorV2` DPI awareness and `longPathAware`. They are wired in
  via each `build.rs` (`tauri_build::WindowsAttributes::new().app_manifest(include_str!(…))`),
  which routes through the same `tauri-winres` resource compiler (windres on the GNU
  toolchain users build with) that already embeds the version info — verified: the rebuilt
  PEs now carry `trustInfo`+`supportedOS`+`dpiAwareness` and keep their version info. This
  hardens the *identity/reputation* surface (and Tauri shipping a bare default manifest was
  a genuine gap); it does not by itself change the behavioral verdict, and the residual ML
  scores wander with each rebuild as described above. The durable remedy remains signing.
- **The stub had *zero* version metadata** (blank CompanyName/ProductName/etc.) — the
  single strongest "anonymous dropper" signal. It now embeds a full `VERSIONINFO`
  resource and a proper Windows application manifest (`asInvoker`, DPI-aware,
  supported-OS) compiled by `windres` in `installer/stub/build.rs`. This already dropped
  one engine (5 → 4).
- **The installer and main app now carry publisher + copyright** in their PE version info
  (`bundle.publisher` / `bundle.copyright` in each `tauri.conf.json`), so they read as
  identifiable software rather than anonymous binaries.
- **The stub no longer stages into a random `%TEMP%` folder.** It unpacks to a named
  per-user directory (`%LOCALAPPDATA%\xConsole-Setup\<pid>`, a deliberate *sibling* of the
  install base `%LOCALAPPDATA%\xConsole` so an uninstall's `rmdir` can't race the running
  staged exe). "Write an executable to %TEMP% and run it" is a specific behavioral
  heuristic; running from a stable, clearly-named folder avoids it.

We deliberately **did not** hide, encrypt, pack, or XOR the embedded payload to dodge the
remaining ML verdicts. That is exactly the technique real malware uses to evade scanners;
it would *lower* trust with reputation systems and is the wrong thing to do. The honest
remedy is signing.

## The durable fix: code signing

A valid Authenticode signature from a certificate that chains to a trusted CA is what
removes these false positives — it gives the binary an identity and lets Microsoft
SmartScreen, Kaspersky KSN, Symantec, etc. accrue reputation for *your publisher* instead
of re-judging every new hash from scratch.

### ⚠️ The catch: the main app is compiled on each user's PC

This is the crux of why Kaspersky kept **closing the running main app** (`xconsole.exe`),
and it is a *structural* consequence of the clone+compile distribution model:

- The CI (`installer-release.yml`) builds and can sign **only `xConsole-Setup.exe`**. The
  **main `xconsole.exe` is compiled locally on every user's machine** by the installer
  (`git clone` → `cargo build --release`). CI signing never touches it.
- That locally-built exe is therefore **unsigned and has a brand-new hash on every machine
  and every update** → permanent **zero reputation**. Kaspersky's behavioral engine
  (System Watcher / HIPS) + cloud reputation (KSN) see an unknown, unsigned binary that
  spawns a hidden child (`node.exe` for the Cursor agent) which immediately opens
  network/SSH — and act on it. **No code change removes this**; the binary is statically
  clean (0/75). The full manifest above helps the *static/ML* surface but does not give
  the *behavioral* engine a trusted identity.
- You **cannot** fix this by signing during the local install: that would require shipping
  your private signing key inside an open-source installer, which is impossible to do
  safely.

**So there are exactly two honest ways to stop Kaspersky from closing the main app:**

1. **Ship a prebuilt, CI-signed `xconsole.exe`** (recommended for users who want zero AV
   friction). Add a CI job that builds the main app on the MSVC runner (it statically links
   WebView2 → single self-contained exe, no loose DLL), signs it with your cert, and
   publishes it as a normal release download — *alongside* the clone+compile installer for
   those who prefer to build from source. A single stable, signed identity accrues KSN /
   SmartScreen reputation across all users instead of resetting per machine. This is the
   only path that durably clears the behavioral verdict on the main app.
2. **Keep clone+compile and have each user trust their own build** — add a Kaspersky
   exclusion for the app folder and/or report the false positive (steps below). Works, but
   it is per-user manual effort and the unsigned binary will keep tripping fresh installs.

### Why a per-user / self-signed certificate does NOT work (and makes it worse)

A natural idea: have the installer generate a *free* certificate on each user's PC and sign
the freshly-compiled `xconsole.exe` with it — a unique per-user cert instead of one shared
one. It is a reasonable thought, but it does not help, for three independent reasons:

1. **Self-signed = untrusted.** A signature only earns AV/SmartScreen trust if its cert
   chains to a **trusted root CA**. A cert generated on the user's machine chains to nothing,
   so Kaspersky/Defender treat the binary as effectively unsigned — sometimes *more*
   suspicious ("signed by an unknown self-issued cert").
2. **Reputation is the whole point, and it's per-identity, not per-binary.** Signing helps
   because a vendor (Kaspersky KSN, Microsoft SmartScreen) accumulates reputation for **one
   publisher identity** across many users and downloads. A *different* cert on every machine
   is the opposite — every install is a brand-new unknown identity with zero reputation.
   That's the same dead end as no signature at all.
3. **Making a self-signed cert "trusted" means injecting a root certificate into the
   Windows trust store — which is itself a textbook malware action.** Kaspersky and Defender
   specifically watch for and flag "app adds a certificate to the Trusted Root / Trusted
   Publishers store." It also needs privileges the installer deliberately does not request
   (it runs non-admin, under the user profile). So this path would *raise* detections and
   add real security risk, while still not buying any reputation.

**Bottom line:** there is no free per-user trusted code-signing certificate (unlike
Let's Encrypt for TLS, no CA issues code-signing certs automatically/free to individuals).
A signature only helps when it is **one** trusted identity shared across all users — i.e.
applied to a **prebuilt** binary in CI, not to a per-user local compile.

> Do **not** generate a self-signed cert and add it to the user's trust store from the
> installer — modifying the certificate store or AV settings is malware-shaped and will
> *increase* detections.

### The genuinely-free durable fix: SignPath Foundation (free for open source)

xConsole is MIT-licensed with a public repo, so it qualifies for **[SignPath Foundation]
(https://signpath.org/)** — they provide **free Authenticode code-signing certificates and a
signing service to qualifying open-source projects**, integrated as a GitHub Actions step.
This is a *real* trusted cert (chains to a trusted CA), at **no cost**. It signs the
**prebuilt CI artifact** — so it pairs with option 1 above: build a single prebuilt
`xconsole.exe` (+ the installer) in CI and have SignPath sign them. One stable signed
identity then accrues KSN/SmartScreen reputation for *all* users, for free. (Azure Trusted
Signing ~$10/mo or an OV cert ~$200-400/yr are the paid alternatives, not required.)

### Certificate options

- **OV (Organization Validation) code-signing cert** (~$200–400/yr, e.g. Sectigo,
  DigiCert). Clears the `!ml` verdicts; reputation builds over a few weeks/downloads.
- **EV (Extended Validation) cert** — instant SmartScreen reputation, highest trust, but
  pricier and requires a hardware token / cloud HSM.
- **Azure Trusted Signing** (~$10/mo, Microsoft) — cheapest path to a trusted signature
  for individuals/small orgs; integrates with `signtool`.
- **Free for OSS:** [SignPath.io](https://signpath.io) has a free tier for open-source
  projects.

> Self-signed certificates do **not** help — they are untrusted, so AV reputation systems
> ignore (or further distrust) them. Don't bother.

### Signing locally

`installer/build-single-exe.ps1` signs automatically when a cert is configured:

```powershell
$env:XCONSOLE_SIGN_PFX      = 'C:\path\to\codesign.pfx'
$env:XCONSOLE_SIGN_PASSWORD = '••••••'
# or, if the cert is already in your certificate store:
# $env:XCONSOLE_SIGN_THUMBPRINT = 'AB12CD...'
.\installer\build-single-exe.ps1
```

It signs the inner installer *before* it is embedded, and the final launcher after — both
end up signed.

### Signing in CI

`.github/workflows/installer-release.yml` has an optional **"Sign the installer"** step
that activates when these repo secrets exist (Settings → Secrets and variables → Actions):

- `WINDOWS_SIGN_PFX_BASE64` — your `.pfx` file, base64-encoded
  (`[Convert]::ToBase64String([IO.File]::ReadAllBytes('codesign.pfx'))`).
- `WINDOWS_SIGN_PASSWORD` — the PFX password (omit if the key has none).

Without the secrets it is a no-op and ships unsigned exactly as before.

## Recommended distribution

For the cleanest result with the least AV friction:

1. **Ship the MSVC build from CI** (the rolling `installer-latest` release). It statically
   links WebView2 into a *single* exe, so it has **no embedded-and-dropped payload** — it
   never gets the stub's `ML.Attribute` / `Elastic` verdicts. The single-exe stub in
   `installer/stub/` exists only to give *local GNU* builds a one-file artifact; it is a
   developer convenience, not the shipping path.
2. **Sign it** (above) to clear `Wacapew.C!ml` and give the main app KSN/SmartScreen
   reputation so Kaspersky stops closing it.
3. If you can't sign yet, the 2-file zip (`xConsole-Setup.exe` + `WebView2Loader.dll`,
   already produced by CI) avoids the dropper verdicts that the single-file stub gets.

## Interim fix when Kaspersky closes the locally-compiled app

Until a prebuilt signed build exists, a user running the clone+compile build can trust it
on their own machine. This is a legitimate, consented action — the user installed the app
and is choosing to trust software they built from public source. **xConsole never does this
for you**; it is a manual step the user performs in their AV's own UI:

- **Kaspersky** has no public API to add an exclusion programmatically (by design). The
  user adds it manually: *Settings → Security settings → Exclusions and trusted apps /
  Threats and Exclusions → Manage exclusions → Add* the install folder
  `%LOCALAPPDATA%\xConsole` (and `%LOCALAPPDATA%\xConsole-Setup`), and/or *Manage trusted
  applications → Add* `xconsole.exe`. Also submit the file to Kaspersky as a false alarm
  (below) so it clears in KSN for everyone.
- **Microsoft Defender** exclusion (user-run, with consent — paste into an elevated
  PowerShell): `Add-MpPreference -ExclusionPath "$env:LOCALAPPDATA\xConsole"`. Defender did
  not statically flag the main app (0/75), so this is rarely needed for it.

## If a vendor still flags a signed build — report the false positive

Submit the binary; vendors whitelist within days and the verdict clears for everyone:

- **Microsoft Defender:** <https://www.microsoft.com/en-us/wdsi/filesubmission> (choose
  "Software developer", "Incorrectly detected as malware").
- **Kaspersky:** <https://opentip.kaspersky.com/> (upload) or
  false-alarm@kaspersky.com / <https://support.kaspersky.com/general/false>.
- **Symantec/Broadcom:** <https://submit.symantec.com/false_positive/>.
- **Elastic:** <https://github.com/elastic/protections-artifacts/issues>.
- **VirusTotal contact tab** on the file's report page links each engine's dispute form.

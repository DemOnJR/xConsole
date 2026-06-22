# xConsole security policy

## Dependency supply-chain hardening

xConsole enforces a **7-day minimum release age** on every dependency (direct and
transitive). Most malicious package releases are detected and yanked within hours,
so a one-week cooldown filters out smash-and-grab supply-chain attacks while still
letting us take security fixes deliberately.

### Frontend (pnpm)

Configured in [`pnpm-workspace.yaml`](pnpm-workspace.yaml):

- `minimumReleaseAge: 10080` (7 days, in minutes)
- `minimumReleaseAgeStrict: true` — installs **fail** rather than silently using a
  too-fresh version.
- `onlyBuiltDependencies` — allow-list for packages permitted to run install scripts
  (pnpm blocks all by default).
- Exact version pins (`save-exact=true` in [`.npmrc`](.npmrc)); no `^`/`~` ranges.
- CI installs with `pnpm install --frozen-lockfile`.

### Backend (Cargo)

- 7-day cooldown via [`cooldown.toml`](cooldown.toml) (`cargo-cooldown`), fail-closed.
- [`src-tauri/deny.toml`](src-tauri/deny.toml) (`cargo-deny`): RustSec advisories,
  license allow-list, and registry/source restrictions.
- `cargo-audit` against the RustSec advisory DB.
- Exact version expectations pinned by a committed `Cargo.lock`; CI builds `--locked`.

### Process rules

- Every new dependency needs a stated reason; prefer the platform/std library or a
  small, well-known crate over a deep tree.
- Dependency-update PRs are never auto-merged.
- CI gates (`--frozen-lockfile` + `--locked` + cooldown + audit) must pass to merge.

## SSH credential handling

- Prefer **ssh-agent** so the app never sees private key bytes.
- Private keys are referenced by **path** only; key material is never copied into the
  local database.
- Passphrases/passwords are stored in the **OS keychain** (`keyring`), never plaintext.
- Decrypted key material is **zeroized** from memory after each connect.
- Host keys are verified (trust-on-first-use `known_hosts`) to prevent MITM.

Report vulnerabilities privately to the maintainers before public disclosure.

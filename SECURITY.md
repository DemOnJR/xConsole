# xConsole security policy

xConsole is **open source**: anyone can read every line of this code. Its security is
designed accordingly — it never depends on the source being secret (Kerckhoffs's
principle). Keys are random, credentials live in the OS keychain rather than in the repo or
the database, and the encryption is standard, public, and auditable.

Where the implementation falls short of the intent, this document says so — see
**Known gaps** under SSH credential handling, and the caveats under at-rest encryption.

## Local-only deployment posture

xConsole is a **single-user desktop application**. It is **not** designed to be deployed
on a public or internet-facing server, and must not be exposed on a public IP address.

- It holds the keys to your infrastructure (SSH credentials, cloud/API keys) and runs an
  AI assistant that can execute shell commands locally and on your servers. It assumes it
  runs on one trusted machine controlled solely by you.
- It does **not** listen on any externally reachable interface. The only background
  services it starts — the local LLM helper (`llama-server`) and the speech-to-text
  server (`whisper`) — bind to `127.0.0.1` (loopback) only, and the in-app MCP server
  speaks over **stdio**, not a network socket. There is nothing to reach from the network.
- All outbound connections (SSH to your servers, AI providers, web tools) are
  client-initiated. The web-fetch/search tools validate target URLs to block requests to
  loopback, link-local, and cloud-metadata addresses (SSRF protection).

## At-rest encryption (optional app lock)

By default, your local database (`xconsole.db`) stores chats, workspaces, settings, and
agent memory in your OS app-data directory. Credentials (SSH passwords and keys, API
tokens) live in the OS keychain, not here. You can additionally encrypt the whole database
at rest by enabling the **app lock** with a master password (Settings → Security):

- A random 256-bit **data key** encrypts the database with **AES-256-GCM** (authenticated
  encryption, via `ring`), using a fresh CSPRNG nonce on every write of the blob. The
  database is encrypted as a whole, not record by record.
- The data key is **wrapped** (encrypted) by a key derived from your master password with
  **PBKDF2-HMAC-SHA256** and a random per-install salt. The wrapped blob lives in a small
  plaintext manifest (`db.lock.json`) and doubles as the password verifier — a wrong
  password simply fails to authenticate.
- "Remember this device" stores the raw data key in the **OS keychain** (DPAPI-protected
  on Windows), so the app unlocks silently on your machine while the database stays
  encrypted on disk.
- **There is no recovery path, by design.** Lose the master password *and* the device key
  and the data is gone — which is exactly what makes a stolen `xconsole.db.enc` useless.
- You can export an unencrypted backup before enabling the lock, and the app keeps the
  encrypted artifact (`.enc`) as the canonical at-rest copy. The temporary pre-migration
  copy made while turning the lock on is deleted as soon as the encrypted database is
  verified, so it never lingers as plaintext.
- The master password must be at least **12 characters** — it is the only thing protecting
  a stolen encrypted database, so longer is strictly better.

**Caveat — plaintext working window:** while the app is *unlocked and running* it operates
on a decrypted working copy on disk. That copy exists for the whole session, and an unclean
shutdown (crash/power loss/kill) leaves it readable until the next clean launch reaps it.
At-rest encryption protects an offline or stolen database **between** clean sessions — not a
live, running, or improperly-closed one.

**Caveat — what can still end up in the database:** credentials proper are in the keychain,
but the database is not secret-free by construction. Agent conversations store tool output
verbatim (`agent_conversation`), so anything a command you approved printed to stdout is
persisted — and sent to your model provider. Approval records store the command that was
run; secret-looking `NAME=value` assignments are masked before storage, but a credential
passed some other way (a positional argument, `-pSECRET`) is not. Treat the database as
sensitive, not merely as metadata.

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

- Private keys are referenced by **path** only; key material is never copied into the
  local database. Keys the app generates itself live only in the OS keychain.
- Passphrases/passwords are stored in the **OS keychain** (`keyring`), never plaintext.
  With **Encrypt saved credentials** on (Settings → Security), they are additionally
  encrypted with the database data key *before* reaching the keychain, so reading the OS
  credential store yields ciphertext rather than credentials.
- Host keys are pinned trust-on-first-use in `known_hosts`, and a later **mismatch fails
  the connection closed**.

### Known gaps

Stated plainly rather than omitted, because a security policy that overstates what the
code does is worse than one that admits a gap:

- **ssh-agent is not supported.** Selecting it returns an error
  (`ConnectError::AgentUnsupported`); use a key file or the app-managed key instead. An
  earlier version of this document claimed agent support as the preferred path.
- **Passwords and passphrases are not zeroized.** They are read from the keychain into a
  `Zeroizing` buffer but then copied into a plain `String` to hand to the SSH client, and
  that copy is not wiped. Only app-managed private-key PEM bytes stay in `Zeroizing`
  end-to-end.
- **First contact with a host is trusted without asking.** The key is pinned silently on
  the first connection, so an attacker in position at that exact moment can pin their own
  key — and for password auth, receive the password. Mismatches afterwards are refused.
- **Credentials are visible in the remote process list.** Cloud credentials for a
  Terraform run on a VPS runner are exported in the command string, so they appear in
  `ps` and execve auditing on that server for the duration of the run.

Report vulnerabilities privately to the maintainers before public disclosure.

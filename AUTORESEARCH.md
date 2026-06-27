# Autoresearch — the self-improving "learn a skill" loop

When the agent needs to do something it doesn't know how to do, it researches the
topic on the public web, synthesizes a reusable `SKILL.md` *grounded only in the
pages it read*, saves it (quarantined), and applies it — learning the capability
itself instead of guessing. Inspired by [karpathy/autoresearch](https://github.com/karpathy/autoresearch)
(an autonomous loop that produces lightweight steering artifacts; here the artifact
is a skill).

This matters most for the **local model** (qwen3.5:9b via Ollama): a 9B confidently
answers niche DevOps questions from memory — often subtly wrong, which is dangerous
when commands run on real servers.

## How it triggers (the important part)

A weak local model will **not** reliably pick a rarely-used `learn_skill` tool out of
~15 on its own. Measured trigger recall across every prompt wording we tried was ~0 —
even for a *fictional* tool it had never heard of, it answered in prose rather than
admitting the gap.

So the reliable trigger is **not** the model self-selecting the tool. It is a
**pre-turn classifier** (`autoresearch::assess_gap`): one cheap, temperature-0
question — *"does this need specific commands/config for a named tool you're unsure
of? name the topic, or say NONE."* A 9B answers a focused, direct question far more
reliably than it spontaneously reaches for a rare tool. Measured: **recall ~0.75,
precision 1.00** (zero false positives on `ls` / math / file edits).

### The autopilot (agent.rs)

On every local, tool-capable, non-casual turn (gated by `agent.learn_autopilot`,
default on):

1. **Classify** — `assess_gap` runs once. If it returns `NONE`, nothing happens (no
   latency beyond one tiny call).
2. **Research** — on a detected gap with no covering skill, `autoresearch::learn`
   runs the full loop (below). The expensive web research only runs on a genuine gap.
3. **Inject** — the resulting skill is appended to the system prompt as
   *"Just-researched skill for this task — APPLY IT"*, and the user sees a
   *"Learned a skill for X — applying it"* status.
4. **Answer** — the model answers using the injected, verified-against-sources steps.

The model can also call the `learn_skill` tool directly, and the reflection pass
writes a `[gap]` memory bullet when the agent visibly declines — but the autopilot
is what makes it dependable.

## The research loop (`autoresearch::learn`)

1. **Dedup** — if an installed skill already covers the topic, return it; skip research.
2. **Sanitize the query** — private IPs, internal hostnames (`.internal`/`.local`/
   `.lan`), the user's own VPS hostnames, credential markers, and high-entropy tokens
   are stripped *before* the query reaches DuckDuckGo. The search topic is the generic
   capability, never the specific incident.
3. **Gather sources** — search, then **fetch the top 1–2 result pages** (load-bearing:
   snippets alone are too thin to ground real commands). All fetches reuse the
   SSRF-guarded `web_tools` path.
4. **Synthesize** — one low-temperature (0.15) call fills a fixed `SKILL.md` skeleton
   **using only the fetched source text**, with an explicit `# TODO: not found in
   sources` escape hatch so it leaves gaps blank instead of confabulating.
5. **Validate, de-fang, scan, save** (`process_synthesized`, a pure function):
   - structural gate (real `description:` front-matter, ≥1 command, cited sources that
     match pages actually fetched, no model prompt-leakage);
   - **de-fang** destructive commands (`rm -rf`, `mkfs`, `dd`, `chmod 777 /`, …) by
     rewriting the line to `# REQUIRES APPROVAL:` — kept, never silently deleted;
   - **security scan** (`commit_candidate`): **NVIDIA SkillSpector** is the primary
     static analyzer when installed (68 patterns / 17 categories: prompt injection,
     exfiltration, privilege escalation, supply chain, dangerous-code AST, YARA, …),
     run static-only (`--no-llm`, no API key); the built-in heuristic is the always-on
     backstop. Both gate at a **stricter threshold** than user-chosen installs (≥40 /
     `is_blocking`, vs 60 for `skill_install`) — a researched skill is more untrusted
     than one the user picked, so pipe-to-shell (`curl … | sh`) is refused outright;
   - **quarantine** under the `unverified/` category with server-authored provenance
     front-matter (`status: draft`, `origin: autoresearch`, `verified: false`,
     `sources: […]`) and an UNVERIFIED banner, **never overwriting** an existing skill.

## Why this is safe

A skill is a file the agent later *follows as trusted instructions*, so web text
laundered into a `SKILL.md` is a prompt-injection / RCE vector. The laundering is
closed at every step: the query never carries private context out; synthesis is
grounded and cold; the output is validated, de-fanged, and scanned at a stricter bar
than installs; it lands in a distinct `unverified/` namespace with a banner so the
distrust label is re-attached every time it's re-injected; and the agent is told never
to run a destructive command from a learned skill without the user's approval.

## The security scanner (NVIDIA SkillSpector)

Every skill — researched, downloaded (`skill_install`), or otherwise — is scanned
before it's saved or installed, because a `SKILL.md` is followed as trusted
instructions. The scanner is **NVIDIA SkillSpector** ([github.com/NVIDIA/skillspector](https://github.com/NVIDIA/skillspector))
when installed, falling back to a built-in pure-Rust heuristic otherwise.

- **Install** (one click in Settings → Security, or):
  `uv tool install git+https://github.com/NVIDIA/skillspector.git` (uv provisions
  the required Python 3.12 automatically). The app finds the executable via
  `uv tool dir --bin` even when it isn't on `PATH`.
- Runs **static-only** by default (`scan … -f json --no-llm`) — no API key, no network
  beyond the optional OSV.dev dependency check.
- **Deep analysis (opt-in)**: Settings → Skills → "Deep analysis with the local model"
  adds SkillSpector's LLM semantic checks against your local Ollama (OpenAI-compatible
  endpoint; nothing leaves the machine). Use a **non-thinking instruct model** (or a
  cloud model) — *thinking* models (qwen3.x) emit long `<think>` traces that overrun
  SkillSpector's completion budget, so a deep scan with them fails and **falls back to
  the static SkillSpector scan** (never down to the weak built-in heuristic). Stored in
  `skills.scanner_deep` / `skills.scanner_model`; the endpoint/model derive from the
  active Ollama provider.
- Verdict: `risk_assessment.{score,severity,recommendation}` + an `issues[]` list.
  Blocking on score ≥ threshold, HIGH/CRITICAL severity, or a `DO_NOT_INSTALL`
  recommendation. Verify with `xconsole-bench scanner` (a malicious sample scores
  71/HIGH/DO_NOT_INSTALL → blocked; a clean one 0/LOW → allowed); `--deep` exercises the
  LLM path.

## Settings

- `agent.learn_autopilot` — pre-turn gap detection + auto-research (default **on**).
- `agent.self_improve` — the reflection pass that writes `[lesson]`/`[gap]` memory
  bullets (default **on**).
- **Skill scanner** — Settings → Security shows whether SkillSpector is active and
  installs it in one click (`skill_scanner_status` / `install_skill_scanner` commands).

## Tested

`xconsole-bench` modes exercise every layer:

- `selftest` — pure, no model/network: injection refused, destructive de-fanged,
  quarantine + no-overwrite, query sanitization, structural validation, classifier
  reply parsing (59 checks).
- `learnclassify` — the gap classifier as a TP/FP/TN/FN confusion matrix.
- `learntune` — A/B sweep of guidance/tool-description variants (how we learned that
  prompt-only triggering doesn't work).
- `learn` — the live full loop on a real topic **and** the autopilot end-to-end
  (gate → research → inject → grounded answer).

Deferred to a future "overnight" pass (v2): promoting `draft → verified` from
execution outcomes, refining a skill that failed in use, proactive research of
recurring `[gap]`s, and a skills dedup/merge pass.

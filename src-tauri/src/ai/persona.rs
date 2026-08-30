//! Named agents: an identity the autonomous goal loop runs under.
//!
//! The goal loop already does the hard part — plan, act, verify, wait, notify when
//! finished. What it had no notion of was *who* was doing the work. Every run was
//! the same anonymous agent with the same servers, the same trust level and the same
//! model, so "have Ada watch the logs while the CEO reviews the migration plan" had
//! no way to exist.
//!
//! A persona supplies the missing identity: a name to address it by, standing
//! instructions, the servers it works on, how far it is trusted, and optionally its
//! own model — routine triage does not need to run on the model that makes
//! architecture calls.

use crate::storage::models::Persona;
use crate::storage::Db;

/// How much of a persona's instructions reach the prompt.
///
/// Generous enough for a real brief, bounded so one verbose persona cannot crowd out
/// the memory, skills and host context that share the window.
const INSTRUCTIONS_MAX_CHARS: usize = 2_000;

/// Look a persona up by id, or — failing that — by name.
///
/// The agent picks a persona from a task description, so it will pass whatever the
/// user typed: "Ada", "ada", or an id it read from a listing. Accepting both means a
/// delegation does not fail on capitalisation.
pub fn resolve(db: &Db, id_or_name: &str) -> Option<Persona> {
    let needle = id_or_name.trim();
    if needle.is_empty() {
        return None;
    }
    if let Ok(Some(p)) = db.get_persona(needle) {
        return Some(p);
    }
    db.get_persona_by_name(needle).ok().flatten()
}

/// The block describing this persona, prepended to a goal cycle's prompt.
///
/// Deliberately short and concrete. A persona is an identity and a remit, not a
/// second system prompt — the soul, memory and safety rules still apply underneath,
/// and restating them here would only give the model room to contradict them.
pub fn prompt_block(persona: &Persona) -> String {
    let mut out = format!("You are {}", persona.name);
    if !persona.role.trim().is_empty() {
        out.push_str(&format!(", {}", persona.role.trim()));
    }
    out.push_str(
        ". You are working on your own, in the background, on a task the user handed \
         to you. Finish it. Only stop to ask the user something if you genuinely \
         cannot proceed without their decision — not to confirm routine steps, and \
         not because something is slow.",
    );
    let instructions = persona.instructions.trim();
    if !instructions.is_empty() {
        out.push_str("\n\nYour standing instructions:\n");
        out.push_str(&crate::ai::text::keep_newest(
            instructions,
            INSTRUCTIONS_MAX_CHARS,
        ));
    }
    out
}

/// The chain-of-command block: who this persona answers to, who answers to it, and
/// the rule that only the top of the chain speaks to the user.
///
/// Without this, every agent would surface its own questions and the user would be
/// interrupted once per agent — the opposite of the point. A report goes to its
/// manager, and the manager decides what is worth passing up.
pub fn hierarchy_block(all: &[Persona], me: &Persona) -> String {
    let mut out = String::from("\n\nChain of command:\n");
    match manager_of(all, me) {
        Some(mgr) => {
            out.push_str(&format!(
                "- You report to {}{}. Use agent_report to tell them anything they need to \
                 know: progress worth reporting, a result, a blocker, or a question only a \
                 human can answer. You must NOT address the user directly — {} decides what \
                 reaches them.\n",
                mgr.name,
                if mgr.role.trim().is_empty() {
                    String::new()
                } else {
                    format!(" ({})", mgr.role.trim())
                },
                mgr.name
            ));
        }
        None => {
            out.push_str(
                "- You report to the user. You are the only one who speaks to them, so \
                 consolidate what your reports tell you rather than forwarding it \
                 piecemeal. agent_report reaches the user.\n",
            );
        }
    }
    let team = reports_of(all, me);
    if !team.is_empty() {
        out.push_str("- Reporting to you:\n");
        for member in &team {
            out.push_str(&format!(
                "  - {}{}\n",
                member.name,
                if member.role.trim().is_empty() {
                    String::new()
                } else {
                    format!(" — {}", member.role.trim())
                }
            ));
        }
        out.push_str(
            "  Hand them work with agent_delegate, ask them things with agent_send, and \
             read what they send back with agent_inbox. Do not do their work yourself \
             when it is squarely theirs.\n",
        );
    }
    out.push_str(
        "- Check agent_inbox at the start of a cycle and after finishing a step.\n\
         - Talk to each other rather than stalling: if you are blocked on something \
         another agent owns, ask them and get on with something else.",
    );
    out
}

/// The safety mode a persona's runs use, falling back to the global default.
///
/// A persona is how the user says "this one may restart services unattended, that one
/// may only look" — which only means anything if the loop actually honours it.
pub fn safety_mode(db: &Db, persona: Option<&Persona>) -> String {
    persona
        .and_then(|p| p.safety_mode.clone())
        .filter(|m| matches!(m.as_str(), "full" | "allowlist" | "approve"))
        .unwrap_or_else(|| crate::ai::safety::global_safety_mode(db))
}

/// The servers a run should act on: the task's targets if it named any, else the
/// persona's defaults.
///
/// A persona is normally tied to the machines it looks after, so delegating to it
/// should not require repeating them; naming targets explicitly still wins.
pub fn effective_targets(persona: Option<&Persona>, requested: &[String]) -> Vec<String> {
    if !requested.is_empty() {
        return requested.to_vec();
    }
    persona.map(|p| p.targets.clone()).unwrap_or_default()
}

/// One line per persona, for a picker or for the agent to choose from.
/// The agents in play for one project: its own team, plus the company-wide ones.
///
/// With several projects running, every agent being visible everywhere makes "the
/// reviewer" ambiguous and gives routing nothing to route on. An agent with no project
/// is deliberately company-wide — that is what the one the user talks to is.
pub fn team_for<'a>(all: &'a [Persona], workspace_id: Option<&str>) -> Vec<&'a Persona> {
    all.iter()
        .filter(|p| match (p.workspace_id.as_deref(), workspace_id) {
            // Company-wide agents are in play everywhere, including with no project open.
            (None, _) => true,
            // A project's own team, only while that project is the one being worked on.
            (Some(home), Some(here)) => home == here,
            // No project open: another project's team is not addressable by accident.
            (Some(_), None) => false,
        })
        .collect()
}

pub fn format_catalog(personas: &[Persona]) -> String {
    if personas.is_empty() {
        return "(no personas defined — create one in Settings → Agents)".into();
    }
    personas
        .iter()
        .filter(|p| p.enabled)
        .map(|p| {
            let role = if p.role.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", p.role.trim())
            };
            let targets = match p.targets.len() {
                0 => String::new(),
                n => format!(" [{n} default target(s)]"),
            };
            format!("- {}{role}{targets}", p.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Org chart
// ---------------------------------------------------------------------------

/// Longest reporting chain we will walk.
///
/// `reports_to` is user-editable and the database cannot express "no cycles", so a
/// chain can be circular (A reports to B reports to A) — and escalation walks it
/// upward. Every walk is bounded, so a cycle degrades to "escalates as far as the
/// bound" instead of hanging the agent that tried to report to its manager.
const MAX_CHAIN: usize = 16;

/// The persona `p` reports to, if it has one.
pub fn manager_of<'a>(all: &'a [Persona], p: &Persona) -> Option<&'a Persona> {
    let id = p.reports_to.as_deref()?;
    all.iter().find(|c| c.id == id)
}

/// Everyone who reports directly to `p`.
pub fn reports_of<'a>(all: &'a [Persona], p: &Persona) -> Vec<&'a Persona> {
    all.iter()
        .filter(|c| c.reports_to.as_deref() == Some(p.id.as_str()) && c.enabled)
        .collect()
}

/// Whether `p` answers to the user directly.
///
/// This is the rule that makes the hierarchy mean something: only a top-level
/// persona may address the user. Everyone else reports to their manager and it is
/// the manager's job to decide what is worth passing on — which is what stops five
/// agents all interrupting about the same task.
pub fn is_top_level(p: &Persona) -> bool {
    p.reports_to.as_deref().map(str::trim).unwrap_or("").is_empty()
}

/// Walk from `p` up to the top, `p` first. Stops at [`MAX_CHAIN`] or on a cycle.
pub fn chain_to_top<'a>(all: &'a [Persona], p: &'a Persona) -> Vec<&'a Persona> {
    let mut out = vec![p];
    let mut seen = vec![p.id.clone()];
    let mut cur = p;
    while out.len() < MAX_CHAIN {
        let Some(next) = manager_of(all, cur) else { break };
        if seen.contains(&next.id) {
            break; // cycle — stop rather than loop
        }
        seen.push(next.id.clone());
        out.push(next);
        cur = next;
    }
    out
}

/// Would setting `persona_id`'s manager to `new_manager_id` create a reporting loop?
///
/// Checked before saving so the org chart cannot be put into a state where an
/// escalation has no top to reach.
pub fn would_create_cycle(all: &[Persona], persona_id: &str, new_manager_id: &str) -> bool {
    if persona_id == new_manager_id {
        return true; // reporting to yourself
    }
    let mut cur = new_manager_id.to_string();
    for _ in 0..MAX_CHAIN {
        if cur == persona_id {
            return true;
        }
        let Some(node) = all.iter().find(|p| p.id == cur) else {
            return false;
        };
        match node.reports_to.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(next) => cur = next.to_string(),
            None => return false,
        }
    }
    // Ran out of chain without reaching the top: the existing structure is already
    // circular, so adding to it cannot be safe.
    true
}

/// The org chart as indented text, for a prompt or a settings panel.
pub fn format_org_chart(all: &[Persona]) -> String {
    let enabled: Vec<&Persona> = all.iter().filter(|p| p.enabled).collect();
    if enabled.is_empty() {
        return "(no agents defined — create one in Settings → Agents)".into();
    }

    fn line(p: &Persona) -> String {
        if p.role.trim().is_empty() {
            p.name.clone()
        } else {
            format!("{} — {}", p.name, p.role.trim())
        }
    }

    fn walk(all: &[Persona], p: &Persona, depth: usize, out: &mut Vec<String>, seen: &mut Vec<String>) {
        if depth > MAX_CHAIN || seen.contains(&p.id) {
            return;
        }
        seen.push(p.id.clone());
        out.push(format!("{}- {}", "  ".repeat(depth), line(p)));
        for child in reports_of(all, p) {
            walk(all, child, depth + 1, out, seen);
        }
    }

    let mut out = Vec::new();
    let mut seen = Vec::new();
    for p in enabled.iter().filter(|p| is_top_level(p)) {
        walk(all, p, 0, &mut out, &mut seen);
    }
    // Anyone whose manager is disabled or missing would otherwise be invisible.
    for p in enabled.iter().filter(|p| !seen.contains(&p.id)) {
        out.push(format!("- {} (manager missing)", line(p)));
    }
    out.join("\n")
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Words too common to say anything about who should take a task.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "this", "that", "from", "into", "you", "your", "our",
    "are", "was", "were", "has", "have", "had", "all", "any", "can", "will", "should",
    "please", "need", "needs", "make", "check", "run", "get", "set", "use", "using",
    "server", "servers", "agent", "task", "work", "when", "what", "who", "how",
];

fn keywords(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !STOP_WORDS.contains(w))
        .map(str::to_string)
        .collect()
}

/// Pick the agent whose remit best matches a task description.
///
/// This is how Grok Bot routes work — an agent that hits something outside its scope
/// scans the other agents' descriptions and forwards to the closest match — and it is
/// what makes delegation usable in practice: the caller describes the *work*, not the
/// org chart, so adding an agent starts attracting the right tasks without anything
/// else being rewritten.
///
/// Scored on how much of each agent's remit the task mentions, normalised by the
/// length of that remit. Without the normalisation an agent with a long rambling
/// description would win everything simply by having more words to match.
/// Returns `None` when nothing matches, so the caller asks rather than guessing.
pub fn best_match<'a>(all: &'a [Persona], task: &str) -> Option<&'a Persona> {
    let task_words = keywords(task);
    if task_words.is_empty() {
        return None;
    }
    let mut best: Option<(&Persona, f32)> = None;
    for p in all.iter().filter(|p| p.enabled) {
        let remit = format!("{} {} {}", p.name, p.role, p.instructions);
        let remit_words = keywords(&remit);
        if remit_words.is_empty() {
            continue;
        }
        let hits = remit_words
            .iter()
            .filter(|w| task_words.contains(w))
            .count();
        if hits == 0 {
            continue;
        }
        // Naming an agent outright is decisive — "ask Ada to…" must reach Ada even if
        // another agent's description happens to overlap the rest of the sentence.
        let named = task_words.iter().any(|w| w.eq_ignore_ascii_case(&p.name.to_lowercase()));
        // A project's own team wins a tie against a company-wide agent whose description
        // happens to overlap. "Fix the checkout" on the CSB project means CSB's engineer,
        // not whoever else has the word "fix" in their remit.
        let homed = if p.workspace_id.is_some() { 0.5 } else { 0.0 };
        let score = hits as f32 / (remit_words.len() as f32).sqrt()
            + if named { 10.0 } else { 0.0 }
            + homed;
        if best.map(|(_, b)| score > b).unwrap_or(true) {
            best = Some((p, score));
        }
    }
    best.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn persona(name: &str) -> Persona {
        Persona {
            id: format!("id-{name}"),
            name: name.into(),
            role: String::new(),
            instructions: String::new(),
            workspace_id: None,
            targets: vec![],
            safety_mode: None,
            provider_id: None,
            model: None,
            enabled: true,
            reports_to: None,
            created_at: None,
            updated_at: None,
        }
    }

    fn team() -> Vec<Persona> {
        let mut dev = persona("Ada");
        dev.role = "programmer".into();
        dev.instructions = "writes and reviews application code, deployments, migrations".into();
        let mut ops = persona("Grace");
        ops.role = "infrastructure".into();
        ops.instructions = "nginx, tls certificates, firewall, disk space, systemd units".into();
        let mut ceo = persona("CEO");
        ceo.role = "decides priorities and reports to the user".into();
        vec![dev, ops, ceo]
    }

    #[test]
    fn routing_picks_the_agent_whose_remit_matches() {
        let all = team();
        assert_eq!(best_match(&all, "renew the tls certificate for nginx").unwrap().name, "Grace");
        assert_eq!(best_match(&all, "review the migrations in the deployment").unwrap().name, "Ada");
    }

    #[test]
    fn naming_an_agent_outright_wins() {
        let all = team();
        // Mentions Grace's territory, but asks for Ada by name.
        let picked = best_match(&all, "Ada, look at the nginx firewall config").unwrap();
        assert_eq!(picked.name, "Ada");
    }

    #[test]
    fn routing_declines_rather_than_guessing() {
        let all = team();
        // Nothing in anyone's remit — the caller should ask, not be handed a coin flip.
        assert!(best_match(&all, "xyzzy plugh").is_none());
        assert!(best_match(&all, "").is_none());
        // Stop words alone carry no signal.
        assert!(best_match(&all, "please can you check the server").is_none());
    }

    #[test]
    fn routing_ignores_disabled_agents() {
        let mut all = team();
        all[1].enabled = false; // Grace is off
        assert!(best_match(&all, "renew the tls certificate for nginx").is_none());
    }

    #[test]
    fn a_verbose_description_does_not_win_everything() {
        // Normalisation exists for this: without it, the agent with the most words
        // would match every task simply by having more chances to overlap.
        let mut focused = persona("Certbot");
        focused.role = "tls".into();
        let mut rambler = persona("Everyone");
        rambler.instructions = (0..80)
            .map(|i| format!("topic{i}"))
            .collect::<Vec<_>>()
            .join(" ")
            + " tls";
        let all = vec![focused, rambler];
        assert_eq!(best_match(&all, "renew tls").unwrap().name, "Certbot");
    }

    /// ceo <- manager <- dev, plus a disabled retiree.
    fn org() -> Vec<Persona> {
        let mut ceo = persona("CEO");
        ceo.role = "reports to the user".into();
        let mut mgr = persona("Manager");
        mgr.reports_to = Some(ceo.id.clone());
        let mut dev = persona("Dev");
        dev.reports_to = Some(mgr.id.clone());
        let mut gone = persona("Retired");
        gone.reports_to = Some(ceo.id.clone());
        gone.enabled = false;
        vec![ceo, mgr, dev, gone]
    }

    #[test]
    fn only_the_top_of_the_chain_may_address_the_user() {
        let all = org();
        assert!(is_top_level(&all[0]), "CEO reports to the user");
        assert!(!is_top_level(&all[1]));
        assert!(!is_top_level(&all[2]));
        // An empty string is "no manager" too — a UI select can produce one.
        let mut blank = persona("X");
        blank.reports_to = Some("   ".into());
        assert!(is_top_level(&blank));
    }

    #[test]
    fn chain_walks_from_a_report_up_to_the_top() {
        let all = org();
        let names: Vec<&str> = chain_to_top(&all, &all[2])
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["Dev", "Manager", "CEO"]);
        // The top's own chain is just itself.
        assert_eq!(chain_to_top(&all, &all[0]).len(), 1);
    }

    #[test]
    fn direct_reports_skip_disabled_agents() {
        let all = org();
        let names: Vec<&str> = reports_of(&all, &all[0]).iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Manager"], "a disabled agent is not a live report");
    }

    #[test]
    fn a_reporting_cycle_terminates_instead_of_hanging() {
        // The database cannot forbid this, so the walk must survive it.
        let mut a = persona("A");
        let mut b = persona("B");
        a.reports_to = Some(b.id.clone());
        b.reports_to = Some(a.id.clone());
        let all = vec![a.clone(), b];
        let chain = chain_to_top(&all, &all[0]);
        assert!(chain.len() <= MAX_CHAIN, "walk must be bounded");
        assert_eq!(chain.len(), 2, "stops as soon as it revisits someone");
    }

    #[test]
    fn cycles_are_refused_before_they_are_saved() {
        let all = org();
        let (ceo, mgr, dev) = (&all[0], &all[1], &all[2]);
        // CEO reporting to its own grandchild would close the loop.
        assert!(would_create_cycle(&all, &ceo.id, &dev.id));
        assert!(would_create_cycle(&all, &ceo.id, &mgr.id));
        // Reporting to yourself.
        assert!(would_create_cycle(&all, &dev.id, &dev.id));
        // Legitimate: Dev moving to report straight to the CEO.
        assert!(!would_create_cycle(&all, &dev.id, &ceo.id));
        // An unknown manager cannot form a loop.
        assert!(!would_create_cycle(&all, &dev.id, "nobody"));
    }

    #[test]
    fn org_chart_is_indented_by_depth() {
        let chart = format_org_chart(&org());
        let lines: Vec<&str> = chart.lines().collect();
        assert_eq!(lines[0], "- CEO — reports to the user");
        assert_eq!(lines[1], "  - Manager");
        assert_eq!(lines[2], "    - Dev");
        assert!(!chart.contains("Retired"), "disabled agents are not shown");
    }

    #[test]
    fn an_agent_whose_manager_vanished_is_still_listed() {
        // Otherwise deleting a manager would silently hide their reports.
        let mut orphan = persona("Orphan");
        orphan.reports_to = Some("deleted-id".into());
        let chart = format_org_chart(&[orphan]);
        assert!(chart.contains("Orphan (manager missing)"), "{chart}");
    }

    #[test]
    fn org_chart_of_a_cycle_still_renders() {
        let mut a = persona("A");
        let mut b = persona("B");
        a.reports_to = Some(b.id.clone());
        b.reports_to = Some(a.id.clone());
        // Neither is top-level, so both fall through to the "manager missing" pass
        // rather than recursing forever.
        let chart = format_org_chart(&[a, b]);
        assert!(chart.contains("A"), "{chart}");
        assert!(chart.contains("B"), "{chart}");
    }

    #[test]
    fn prompt_names_the_persona_and_tells_it_not_to_check_in() {
        let mut p = persona("Ada");
        p.role = "infrastructure lead".into();
        let block = prompt_block(&p);
        assert!(block.starts_with("You are Ada, infrastructure lead."), "{block}");
        // The whole point of a background persona.
        assert!(block.contains("in the background"), "{block}");
        assert!(block.contains("cannot proceed without their decision"), "{block}");
    }

    #[test]
    fn prompt_works_without_a_role() {
        let block = prompt_block(&persona("CEO"));
        assert!(block.starts_with("You are CEO."), "{block}");
        // No stray comma where the role would have been.
        assert!(!block.contains("CEO,"), "{block}");
    }

    #[test]
    fn prompt_includes_standing_instructions_and_bounds_them() {
        let mut p = persona("Ada");
        p.instructions = "always check systemctl status first".into();
        assert!(prompt_block(&p).contains("always check systemctl status first"));

        p.instructions = "x".repeat(INSTRUCTIONS_MAX_CHARS * 2);
        let block = prompt_block(&p);
        // One verbose persona must not crowd out memory, skills and host context.
        assert!(block.len() < INSTRUCTIONS_MAX_CHARS * 2, "{}", block.len());
    }

    #[test]
    fn safety_falls_back_and_rejects_nonsense() {
        // A mode outside the known set must not silently become policy.
        let mut p = persona("Ada");
        p.safety_mode = Some("yolo".into());
        assert_eq!(
            safety_mode_for_test(Some(&p), "approve"),
            "approve",
            "unknown mode must fall back, not be trusted"
        );
        p.safety_mode = Some("full".into());
        assert_eq!(safety_mode_for_test(Some(&p), "approve"), "full");
        assert_eq!(safety_mode_for_test(None, "allowlist"), "allowlist");
    }

    /// `safety_mode` needs a Db; this mirrors its logic against an explicit default
    /// so the validation rule can be tested without one.
    fn safety_mode_for_test(persona: Option<&Persona>, global: &str) -> String {
        persona
            .and_then(|p| p.safety_mode.clone())
            .filter(|m| matches!(m.as_str(), "full" | "allowlist" | "approve"))
            .unwrap_or_else(|| global.to_string())
    }

    #[test]
    fn explicit_targets_win_over_persona_defaults() {
        let mut p = persona("Ada");
        p.targets = vec!["default-a".into()];
        assert_eq!(
            effective_targets(Some(&p), &["asked-for".to_string()]),
            vec!["asked-for".to_string()]
        );
        assert_eq!(
            effective_targets(Some(&p), &[]),
            vec!["default-a".to_string()]
        );
        assert!(effective_targets(None, &[]).is_empty());
    }

    fn on_project(name: &str, ws: Option<&str>) -> Persona {
        let mut p = persona(name);
        p.workspace_id = ws.map(str::to_string);
        p
    }

    #[test]
    fn a_project_sees_its_own_team_and_the_company_wide_agents() {
        // One team per project. Another project's engineer must not be addressable by
        // accident: with several projects running, "ask the engineer" would otherwise
        // reach whichever one the database returned first.
        let all = vec![
            on_project("Atlas", None),
            on_project("CsbEngineer", Some("ws-csb")),
            on_project("GqEngineer", Some("ws-gq")),
        ];
        let names = |ws| {
            team_for(&all, ws)
                .iter()
                .map(|p| p.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(Some("ws-csb")), vec!["Atlas", "CsbEngineer"]);
        assert_eq!(names(Some("ws-gq")), vec!["Atlas", "GqEngineer"]);
        // With no project open only the company-wide agents answer — the ones that are
        // meant to, rather than every team at once.
        assert_eq!(names(None), vec!["Atlas"]);
    }

    #[test]
    fn the_projects_own_agent_wins_a_tie_against_a_company_wide_one() {
        // Both remits match the words; the one that lives on this project is the one
        // that knows the codebase.
        let mut house = on_project("Fixer", None);
        house.role = "fixes checkout bugs".into();
        let mut mine = on_project("CsbFixer", Some("ws-csb"));
        mine.role = "fixes checkout bugs".into();
        let all = vec![house, mine];
        let team: Vec<Persona> = team_for(&all, Some("ws-csb")).into_iter().cloned().collect();
        assert_eq!(best_match(&team, "fixes checkout bugs").map(|p| p.name.as_str()), Some("CsbFixer"));
    }

    #[test]
    fn catalog_lists_only_enabled_personas() {
        let mut ada = persona("Ada");
        ada.role = "infra".into();
        ada.targets = vec!["v1".into(), "v2".into()];
        let mut off = persona("Retired");
        off.enabled = false;

        let out = format_catalog(&[ada, off]);
        assert!(out.contains("- Ada — infra [2 default target(s)]"), "{out}");
        assert!(!out.contains("Retired"), "{out}");
        assert!(format_catalog(&[]).contains("no personas defined"));
    }
}

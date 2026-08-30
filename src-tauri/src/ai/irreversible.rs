//! Commands that cannot be undone, and what they would destroy.
//!
//! An agent that deletes the wrong thing is not a bug you fix afterwards — the data is
//! gone, and the only question left is what the backup covers. So a command in this
//! class is never simply run: its *blast radius* is measured first, with a read-only
//! command against the same target, and that measurement is what a person is shown
//! before they approve it.
//!
//! The preview is the point. "Delete /srv/app" tells a reader nothing; "Delete
//! /srv/app — 41,203 files, 12 GB, last written four minutes ago" is a different
//! decision, and it is the one that catches a path with a typo in it.
//!
//! # What this deliberately is not
//!
//! Not a security boundary. Anything here can be spelled around by a determined caller,
//! and it makes no attempt to stop that — an agent that wants to destroy something can
//! always write it differently. It is a guard against *mistakes*: the wrong path, the
//! forgotten WHERE clause, the prune that took more than it looked like. Those are what
//! actually happen, and they are worth catching even though the malicious case is not.

/// A command that destroys something, and how to look before leaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irreversible {
    /// What it would destroy, in the words the user would use.
    pub what: String,
    /// A read-only command that measures the damage first. Empty when nothing sensible
    /// can be measured — which is itself worth saying, rather than implying safety.
    pub preview: String,
    /// Why this cannot be undone, for the approval prompt.
    pub why: &'static str,
}

/// Split a command tail into arguments, respecting quotes.
///
/// Splitting on whitespace turned `rm -rf '/srv/my app'` into `/srv/my`, and the
/// preview then measured a path that does not exist — which reads as "nothing there,
/// safe to delete" about a directory that is very much there. A preview of the wrong
/// target is worse than no preview, because it is believed.
fn args(after: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in after.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, c @ ('\'' | '"')) => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            // A separator ends the command; anything after it is a different one.
            (None, ';' | '&' | '|') => break,
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The first argument that looks like a path rather than a flag.
fn first_path(after: &str) -> Option<String> {
    args(after).into_iter().find(|w| !w.starts_with('-'))
}

/// Quote a path for the preview command. The path came from the model, so it is not
/// pasted raw into a shell.
fn q(path: &str) -> String {
    crate::ssh::remote_ops::shell_quote(path)
}

/// Decide whether a command destroys something, and how to measure it first.
///
/// Matches on the lowercased text, so it sees `RM -RF` and `Drop Database` too. It is
/// deliberately generous about what counts: a false positive costs one extra read-only
/// command and a fuller approval prompt, while a false negative costs the data.
pub fn classify(command: &str) -> Option<Irreversible> {
    let lower = command.to_lowercase();
    let trimmed = lower.trim();

    // Recursive delete. The single most common way an agent destroys something it did
    // not mean to, because one wrong path looks exactly like the right one.
    if let Some(idx) = trimmed.find("rm ") {
        let after = &command[idx + 3..];
        let flags: String = after.split_whitespace().take_while(|w| w.starts_with('-')).collect();
        if flags.contains('r') || flags.contains("-recursive") {
            let path = first_path(after).unwrap_or_default();
            if !path.is_empty() {
                return Some(Irreversible {
                    what: format!("delete {path} and everything under it"),
                    // Size and count first, then a sample: the count catches a path that
                    // is far bigger than intended, the sample catches the wrong path.
                    preview: format!(
                        "p={}; echo \"exists: $([ -e \\\"$p\\\" ] && echo yes || echo no)\"; \
                         echo \"size: $(du -sh \\\"$p\\\" 2>/dev/null | cut -f1)\"; \
                         echo \"files: $(find \\\"$p\\\" 2>/dev/null | wc -l)\"; \
                         echo '--- newest 10 ---'; \
                         find \"$p\" -type f -printf '%T@ %p\\n' 2>/dev/null | sort -rn | head -10 | cut -d' ' -f2-",
                        q(&path)
                    ),
                    why: "deleted files are not in a bin; only a backup brings them back",
                });
            }
        }
    }

    // Whole databases and tables.
    for (needle, noun) in [("drop database", "database"), ("drop schema", "schema")] {
        if let Some(idx) = trimmed.find(needle) {
            let name = first_path(&command[idx + needle.len()..])
                .unwrap_or_default()
                .trim_end_matches(';')
                .to_string();
            return Some(Irreversible {
                what: format!("drop the {noun} {name}"),
                preview: format!(
                    "echo 'tables and row counts in {name}:'; \
                     mysql -N -e \"SELECT table_name, table_rows FROM information_schema.tables \
                     WHERE table_schema='{name}' ORDER BY table_rows DESC LIMIT 20\" 2>/dev/null \
                     || psql -d {name} -c '\\dt+' 2>/dev/null \
                     || echo '(could not read it — check by hand before approving)'"
                ),
                why: "a dropped database takes every table with it, in one statement",
            });
        }
    }

    if trimmed.contains("drop table") || trimmed.contains("truncate table") || trimmed.starts_with("truncate ") {
        let verb = if trimmed.contains("drop table") { "drop" } else { "empty" };
        return Some(Irreversible {
            what: format!("{verb} a table"),
            preview: String::new(),
            why: "the rows are gone the moment it runs; there is no transaction to roll back",
        });
    }

    // A DELETE with no WHERE empties the table, and reads as an ordinary statement.
    if trimmed.contains("delete from") && !trimmed.contains("where") {
        return Some(Irreversible {
            what: "delete every row in a table (no WHERE clause)".into(),
            preview: String::new(),
            why: "without a WHERE clause this is the whole table, which is rarely what was meant",
        });
    }

    // Docker's prunes take more than they appear to.
    if trimmed.contains("docker volume rm")
        || trimmed.contains("docker volume prune")
        || (trimmed.contains("docker system prune") && trimmed.contains("--volumes"))
    {
        return Some(Irreversible {
            what: "remove Docker volumes".into(),
            preview: "docker volume ls; echo '--- in use by ---'; docker ps -a --format '{{.Names}} {{.Mounts}}'"
                .into(),
            why: "a volume holds a container's data, and a prune takes every unused one at once",
        });
    }

    // Kubernetes deletes, which can take a whole namespace.
    if trimmed.contains("kubectl delete") {
        return Some(Irreversible {
            what: "delete Kubernetes resources".into(),
            preview: command
                .replace("delete", "get")
                .split(" --")
                .next()
                .unwrap_or(command)
                .to_string(),
            why: "deleting a namespace or a PVC takes its data with it",
        });
    }

    // Disks and devices. Nothing survives these.
    if trimmed.starts_with("mkfs")
        || trimmed.contains(" mkfs.")
        || (trimmed.contains("dd ") && trimmed.contains("of=/dev/"))
        || trimmed.contains("shred ")
    {
        return Some(Irreversible {
            what: "overwrite a block device".into(),
            preview: "lsblk -o NAME,SIZE,MOUNTPOINT,LABEL; echo '--- mounted ---'; df -h".into(),
            why: "this writes over the device itself; there is nothing left to recover from",
        });
    }

    // Git operations that discard work rather than record it.
    if trimmed.contains("push --force") || trimmed.contains("push -f") {
        return Some(Irreversible {
            what: "force-push, overwriting the remote branch".into(),
            preview: "git log --oneline -10 @{u}..HEAD; echo '--- would be dropped ---'; git log --oneline -10 HEAD..@{u}"
                .into(),
            why: "commits only on the remote are erased, including other people's",
        });
    }
    if trimmed.contains("reset --hard") || trimmed.contains("git clean -f") {
        return Some(Irreversible {
            what: "discard uncommitted work in the working tree".into(),
            preview: "git status --short; echo '--- stashes ---'; git stash list".into(),
            why: "uncommitted changes are not recorded anywhere and cannot be recovered",
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads better in a table of assertions than `classify(x).is_some()`.
    fn classify_is(command: &str) -> bool {
        classify(command).is_some()
    }

    #[test]
    fn a_recursive_delete_is_caught_and_measured_first() {
        // The most common way an agent destroys something it did not mean to: one wrong
        // path looks exactly like the right one, and the size is what gives it away.
        let c = classify("rm -rf /srv/app/data").expect("recursive delete is irreversible");
        assert!(c.what.contains("/srv/app/data"), "{}", c.what);
        assert!(c.preview.contains("du -sh"), "should measure the size: {}", c.preview);
        assert!(c.preview.contains("find"), "should count the files: {}", c.preview);
    }

    #[test]
    fn the_flags_are_read_not_the_letters_of_the_path() {
        // `rm -f file` deletes one named file, which is ordinary. `-r` is what makes it
        // a whole tree, and treating every `rm` as irreversible would make the guard
        // noise everyone learns to click through.
        assert!(classify("rm -rf /tmp/x").is_some());
        assert!(classify("rm -fr /tmp/x").is_some());
        assert!(classify("rm --recursive /tmp/x").is_some());
        assert!(classify("rm -f /tmp/one-file.log").is_none());
        assert!(classify("rm /tmp/one-file.log").is_none());
    }

    #[test]
    fn a_path_with_a_space_is_taken_whole_and_quoted() {
        // Splitting on whitespace measured `/srv/my` instead — a path that does not
        // exist, which reads as "nothing there, safe to delete" about a directory that
        // very much is. A preview of the wrong target is worse than none: it is
        // believed.
        let c = classify("rm -rf '/srv/my app'").expect("still a delete");
        assert!(c.what.contains("/srv/my app"), "path was truncated: {}", c.what);
        // And it reaches the preview as one quoted argument, not as shell to run.
        assert!(c.preview.contains("'/srv/my app'"), "{}", c.preview);
    }

    #[test]
    fn a_second_command_on_the_line_is_not_mistaken_for_the_target() {
        // `rm -rf /tmp/x; echo hi` deletes /tmp/x. Reading `echo` or `hi` as the path
        // would preview the wrong thing entirely.
        let c = classify("rm -rf /tmp/x; echo hi").expect("still a delete");
        assert!(c.what.contains("/tmp/x"), "{}", c.what);
        assert!(!c.what.contains("echo"), "{}", c.what);
    }

    #[test]
    fn a_hostile_path_cannot_break_out_of_the_preview() {
        // The path comes from the model, so it is quoted rather than pasted.
        let c = classify("rm -rf \"/srv/a'; rm -rf /\"").expect("still a delete");
        assert!(c.preview.contains("'\\''"), "single quote not escaped: {}", c.preview);
    }

    #[test]
    fn dropping_a_database_lists_what_is_in_it() {
        let c = classify("DROP DATABASE QBCore_88F269;").expect("drop is irreversible");
        assert!(c.what.contains("qbcore_88f269") || c.what.contains("QBCore_88F269"), "{}", c.what);
        assert!(c.preview.contains("table_rows"), "should count rows: {}", c.preview);
    }

    #[test]
    fn a_delete_without_a_where_clause_is_the_whole_table() {
        // Reads like an ordinary statement and empties the table.
        assert!(classify("DELETE FROM orders").is_some());
        assert!(classify("delete from orders;").is_some());
        // With a WHERE it is an ordinary edit, and flagging it would train people to
        // ignore the flag.
        assert!(classify("DELETE FROM orders WHERE id = 42").is_none());
    }

    #[test]
    fn force_push_and_hard_reset_count_as_destroying_work() {
        // Both erase work rather than recording it, which is the failure the whole
        // never-lose-work guarantee exists to prevent.
        assert!(classify_is("git push --force origin main"));
        assert!(classify_is("git push -f"));
        assert!(classify_is("git reset --hard HEAD~3"));
        assert!(classify_is("git clean -fd"));
        // Ordinary git is not flagged.
        assert!(!classify_is("git push origin dev"));
        assert!(!classify_is("git status"));
    }

    #[test]
    fn writing_over_a_device_is_caught() {
        assert!(classify_is("mkfs.ext4 /dev/sdb1"));
        assert!(classify_is("dd if=/dev/zero of=/dev/sda bs=1M"));
        // Reading a device is not destroying it.
        assert!(!classify_is("dd if=/dev/sda of=/backup/disk.img"));
    }

    #[test]
    fn ordinary_work_is_not_flagged() {
        // A guard that fires on everything is a guard nobody reads.
        for cmd in [
            "ls -la /srv",
            "systemctl restart nginx",
            "docker compose up -d",
            "git commit -am 'fix'",
            "SELECT * FROM orders WHERE id = 1",
            "cp -r /srv/a /srv/b",
        ] {
            assert!(!classify_is(cmd), "{cmd} should not be flagged");
        }
    }

    #[test]
    fn every_flagged_command_explains_why_it_cannot_be_undone() {
        // The reason is what a person actually weighs when approving, and an empty one
        // would make the prompt a rubber stamp.
        for cmd in [
            "rm -rf /srv/app",
            "DROP DATABASE x",
            "docker volume prune",
            "git push --force",
            "mkfs.ext4 /dev/sdb",
        ] {
            let c = classify(cmd).expect(cmd);
            assert!(!c.why.trim().is_empty(), "{cmd} has no reason");
            assert!(!c.what.trim().is_empty(), "{cmd} says nothing about what it destroys");
        }
    }
}

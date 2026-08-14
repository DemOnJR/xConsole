//! In-turn working checklist (Claude's TodoWrite).
//!
//! `present_plan` is the user gate at the *start* of big/destructive work.
//! This list is the agent's own scratch pad *while* it executes — one
//! `in_progress` item, mark done as you go — so it does not re-discover
//! finished steps.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub content: String,
    #[serde(default, alias = "activeForm")]
    pub active_form: String,
    pub status: String,
}

impl TodoItem {
    fn normalize(mut self) -> Option<Self> {
        self.content = self.content.trim().to_string();
        if self.content.is_empty() {
            return None;
        }
        self.active_form = self.active_form.trim().to_string();
        if self.active_form.is_empty() {
            self.active_form = self.content.clone();
        }
        let st = self.status.trim().to_ascii_lowercase();
        self.status = match st.as_str() {
            "completed" | "done" => "completed".into(),
            "in_progress" | "doing" | "active" => "in_progress".into(),
            _ => "pending".into(),
        };
        Some(self)
    }
}

/// Replace the session list. At most one item stays `in_progress` (the last
/// one the model marked that way).
pub fn normalize_list(items: Vec<TodoItem>) -> Vec<TodoItem> {
    let mut out: Vec<TodoItem> = items.into_iter().filter_map(TodoItem::normalize).collect();
    let last_active = out.iter().rposition(|t| t.status == "in_progress");
    if let Some(keep) = last_active {
        for (i, t) in out.iter_mut().enumerate() {
            if t.status == "in_progress" && i != keep {
                t.status = "pending".into();
            }
        }
    }
    out
}

pub fn format_block(items: &[TodoItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut lines = vec![
        "# Todos (your working checklist — do not redo a completed item)".to_string(),
        "Keep exactly one item in_progress. Mark it completed when finished, then start the next. \
         This is NOT present_plan — the user already sees this list; just execute."
            .to_string(),
    ];
    for t in items {
        let label = if t.status == "in_progress" && !t.active_form.is_empty() {
            t.active_form.as_str()
        } else {
            t.content.as_str()
        };
        lines.push(format!("[{}] {label}", t.status));
    }
    Some(lines.join("\n"))
}

pub fn format_activity(items: &[TodoItem]) -> String {
    if items.is_empty() {
        return "(empty checklist)".into();
    }
    items
        .iter()
        .map(|t| {
            let mark = match t.status.as_str() {
                "completed" => "x",
                "in_progress" => ">",
                _ => " ",
            };
            format!("[{mark}] {}", t.content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str, status: &str) -> TodoItem {
        TodoItem {
            content: content.into(),
            active_form: String::new(),
            status: status.into(),
        }
    }

    #[test]
    fn keeps_only_last_in_progress() {
        let out = normalize_list(vec![
            item("a", "in_progress"),
            item("b", "in_progress"),
            item("c", "pending"),
        ]);
        assert_eq!(out[0].status, "pending");
        assert_eq!(out[1].status, "in_progress");
        assert_eq!(out[2].status, "pending");
    }

    #[test]
    fn format_mentions_active_form() {
        let items = vec![TodoItem {
            content: "Install cowrie".into(),
            active_form: "Installing cowrie".into(),
            status: "in_progress".into(),
        }];
        let block = format_block(&items).unwrap();
        assert!(block.contains("Installing cowrie"));
        assert!(block.contains("[in_progress]"));
    }
}

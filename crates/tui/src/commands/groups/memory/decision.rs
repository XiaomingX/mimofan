//! `/decision` slash command — durable decision log with an immutable audit trail.
//!
//! Backed by `crates/tui/src/memory.rs`'s structured decision pages
//! (`decisions.md`): each decision keeps a rewritable `current` understanding
//! plus an append-only `history` of `Decision`/`Revision`/`Reversal` events.
//! Unlike conversation bullets, decisions survive compaction — they live in
//! their own file and are never swept by the summary.
//!
//! Subcommands:
//! - `/decision` or `/decision list` — list all decisions (title + current)
//! - `/decision show <id>` — show one decision with its full audit history
//! - `/decision create <id> <title> <current>` — capture a new decision
//! - `/decision revise <id> <new-current>` — update the current understanding
//! - `/decision reverse <id> <why>` — overturn (frozen, history preserved)
//! - `/decision help` — this help

use super::CommandResult;
use crate::memory::{decision_create, decision_reverse, decision_revise, read_decisions};
use crate::tui::app::App;

const DECISION_USAGE: &str = "/decision [list|show <id>|create <id> <title> <current>|revise <id> <new>|reverse <id> <why>|help]";

fn decision_help(dir: &std::path::Path) -> String {
    format!(
        "Manage durable decisions with an immutable audit trail.\n\n\
         Usage: {DECISION_USAGE}\n\n\
         Decisions file: {}\n\n\
         Subcommands:\n\
           /decision                    List all decisions\n\
           /decision list               Same as no-arg\n\
           /decision show <id>          Show one decision + history\n\
           /decision create <id> <title> <current>\n\
                                        Capture a new decision\n\
           /decision revise <id> <new>  Update the current understanding\n\
           /decision reverse <id> <why> Overturn (frozen, history kept)\n\
           /decision help               Show this help\n\n\
         Unlike `# bullet` memories, decisions are never summarized away — they\n\
         persist in `decisions.md` so a future session can see *why* a choice\n\
         was made and later revised or overturned.",
        dir.display()
    )
}

/// Render one decision's audit trail into a readable block.
fn render_entry(e: &crate::memory::DecisionEntry) -> String {
    let mut out = format!(
        "{}  (id: {})  [{}]\n  current: {}\n  created: {}\n  updated: {}{}\n",
        e.title,
        e.id,
        e.category,
        e.current.replace('\n', "\n  "),
        e.created,
        e.updated,
        if e.reversed {
            "\n  STATUS: reversed"
        } else {
            ""
        }
    );
    if !e.history.is_empty() {
        out.push_str("  history:\n");
        for ev in &e.history {
            out.push_str(&format!(
                "    - {} {} — {} ({})\n",
                ev.time,
                ev.kind.as_str(),
                ev.summary,
                ev.source
            ));
        }
    }
    out
}

pub fn decision(app: &mut App, arg: Option<&str>) -> CommandResult {
    if !app.use_memory {
        return CommandResult::error(
            "decision log requires user memory. Enable with `[memory] enabled = true` in `~/.mimofan/config.toml` or `MIMOFAN_MEMORY=on` in your environment, then restart the TUI.",
        );
    }

    let dir = app.memory_dir.clone();
    let sub = arg.unwrap_or("list").trim();

    match sub {
        "help" => CommandResult::message(decision_help(&dir)),
        "" | "list" => {
            let entries = read_decisions(&dir);
            if entries.is_empty() {
                CommandResult::message(
                    "no decisions recorded yet. Create one with `/decision create <id> <title> <current>`.",
                )
            } else {
                let body = entries
                    .iter()
                    .map(|e| format!("- [{}] {}  (updated {})", e.id, e.title, e.updated))
                    .collect::<Vec<_>>()
                    .join("\n");
                CommandResult::message(format!("durable decisions:\n{body}"))
            }
        }
        _ if sub.starts_with("show ") => {
            let id = sub.trim_start_matches("show").trim();
            let entries = read_decisions(&dir);
            match entries.iter().find(|e| e.id == id) {
                Some(e) => CommandResult::message(render_entry(e)),
                None => CommandResult::error(format!("decision `{id}` not found.")),
            }
        }
        _ if sub.starts_with("create ") => {
            let rest = sub.trim_start_matches("create").trim();
            let mut parts = rest.splitn(3, ' ');
            let id = parts.next().unwrap_or_default();
            let title = parts.next().unwrap_or_default();
            let current = parts.next().unwrap_or_default();
            if id.is_empty() || title.is_empty() || current.is_empty() {
                return CommandResult::error("usage: /decision create <id> <title> <current>");
            }
            match decision_create(&dir, id, title, "decision", current) {
                Ok(()) => CommandResult::message(format!("decision `{id}` created.")),
                Err(e) => CommandResult::error(format!("create failed: {e}")),
            }
        }
        _ if sub.starts_with("revise ") => {
            let rest = sub.trim_start_matches("revise").trim();
            let mut parts = rest.splitn(2, ' ');
            let id = parts.next().unwrap_or_default();
            let new_current = parts.next().unwrap_or_default();
            if id.is_empty() || new_current.is_empty() {
                return CommandResult::error("usage: /decision revise <id> <new-current>");
            }
            match decision_revise(&dir, id, new_current, "revised via /decision") {
                Ok(true) => CommandResult::message(format!("decision `{id}` revised.")),
                Ok(false) => {
                    CommandResult::error(format!("decision `{id}` not found or already reversed."))
                }
                Err(e) => CommandResult::error(format!("revise failed: {e}")),
            }
        }
        _ if sub.starts_with("reverse ") => {
            let rest = sub.trim_start_matches("reverse").trim();
            let mut parts = rest.splitn(2, ' ');
            let id = parts.next().unwrap_or_default();
            let why = parts.next().unwrap_or_default();
            if id.is_empty() || why.is_empty() {
                return CommandResult::error("usage: /decision reverse <id> <why>");
            }
            match decision_reverse(&dir, id, why) {
                Ok(true) => CommandResult::message(format!("decision `{id}` reversed (frozen).")),
                Ok(false) => {
                    CommandResult::error(format!("decision `{id}` not found or already reversed."))
                }
                Err(e) => CommandResult::error(format!("reverse failed: {e}")),
            }
        }
        other => CommandResult::error(format!(
            "unknown subcommand `{}`. Try `/decision help`.",
            other
        )),
    }
}

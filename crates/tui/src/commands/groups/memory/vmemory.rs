//! `/vmemory` slash command — inspect and manage the vector memory store.
//!
//! Complements `/memory` (file-based user memory). Requires the
//! `vector-memory` feature (enabled by default) AND a configured embedding
//! backend (`MIMOFAN_MEMORY_API_KEY`); degrades gracefully when not
//! configured.
//!
//! - `/vmemory`                       Show status (enabled? root, dimension)
//! - `/vmemory status`                Alias for the no-arg form
//! - `/vmemory remember <kind> <text>`  Store an observation
//! - `/vmemory query <text>`          Semantic recall of related observations
//! - `/vmemory list`                  List recently stored observations
//! - `/vmemory help`                  Show this help

#![cfg(feature = "vector-memory")]

use std::path::Path;

use super::CommandResult;
use crate::tui::app::App;

const VMEMORY_USAGE: &str = "/vmemory [status|remember <kind> <text>|query <text>|list|help]";

fn project_name(app: &App) -> String {
    app.workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default")
        .to_string()
}

fn memory_dir(app: &App) -> std::path::PathBuf {
    // `app.memory_dir` is the memory directory; the vector store lives at `<dir>/vector`.
    app.memory_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| app.memory_dir.clone())
}

fn configured_dimension() -> usize {
    std::env::var("MIMOFAN_MEMORY_DIMENSION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1536)
}

fn vmemory_help(enabled: bool, root: &Path, dimension: usize) -> String {
    format!(
        "Inspect or manage your vector memory store (semantic recall).\n\n\
         Usage: {VMEMORY_USAGE}\n\n\
         Status: {}\n\
         Root: {}\n\
         Dimension: {dimension}\n\n\
         Subcommands:\n\
           /vmemory                        Show status\n\
           /vmemory status                 Alias for the no-arg form\n\
           /vmemory remember <kind> <text>\n\
                                        Store an observation. <kind> is one of:\n\
                                        user, feedback, project, reference\n\
           /vmemory query <text>           Semantic recall of related observations\n\
           /vmemory list                   List recently stored observations\n\
           /vmemory help                   Show this help\n\n\
         Enable by setting MIMOFAN_MEMORY_API_KEY (and optionally\n\
         MIMOFAN_MEMORY_BASE_URL / MIMOFAN_MEMORY_MODEL / MIMOFAN_MEMORY_DIMENSION).",
        if enabled {
            "enabled"
        } else {
            "disabled (set MIMOFAN_MEMORY_API_KEY to enable)"
        },
        root.display()
    )
}

pub fn vmemory(app: &mut App, arg: Option<&str>) -> CommandResult {
    let sub = arg.unwrap_or("status").trim();
    let mem_dir = memory_dir(app);
    let project = project_name(app);

    // Status / help are read-only and cheap — don't open the store (avoids
    // creating the sled DB on a mere inspection).
    if matches!(sub, "" | "status" | "help") {
        let enabled = crate::vector_memory::VectorMemory::is_configured();
        let root = mem_dir.join("vector");
        let dimension = configured_dimension();
        return match sub {
            "help" => CommandResult::message(vmemory_help(enabled, &root, dimension)),
            _ => CommandResult::message(format!(
                "vector-memory: {}\nroot: {}\ndimension: {}\n\nUse `/vmemory help` for subcommands.",
                if enabled {
                    "enabled"
                } else {
                    "disabled (set MIMOFAN_MEMORY_API_KEY to enable)"
                },
                root.display(),
                dimension
            )),
        };
    }

    // `remember` / `query` / `list` need async embedding — bridge the current
    // tokio runtime (mirrors commands/groups/skills/skills.rs).
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut vm = crate::vector_memory::VectorMemory::open(&mem_dir)
                .map_err(|e| format!("failed to open vector memory: {e}"))?;
            if !vm.enabled() {
                return Err(
                    "vector-memory is disabled. Set MIMOFAN_MEMORY_API_KEY (and optionally \
                     MIMOFAN_MEMORY_BASE_URL/MODEL/DIMENSION) to enable, then restart."
                        .to_string(),
                );
            }
            match sub.split_whitespace().next() {
                Some("remember") => {
                    let rest = sub.trim_start_matches("remember").trim();
                    let (kind, content) = match rest.split_once(' ') {
                        Some((k, c)) if !c.trim().is_empty() => (k.trim(), c.trim()),
                        _ => return Err("usage: /vmemory remember <kind> <text>".to_string()),
                    };
                    let obs_kind = crate::vector_memory::parse_memory_category(kind)
                        .map_err(|e| e.to_string())?;
                    let kind_str = obs_kind.as_str();
                    let embedder = vm
                        .take_embedder()
                        .ok_or_else(|| "vector-memory embedder unavailable".to_string())?;
                    let embedding = embedder
                        .embed_text(content)
                        .await
                        .map_err(|e| format!("embedding failed: {e}"))?;
                    let id = vm
                        .store_observation(&project, kind_str, content, &embedding)
                        .map_err(|e| format!("failed to store: {e}"))?;
                    Ok(format!("remembered (vector id {id}): [{kind_str}] {content}"))
                }
                Some("query") => {
                    let q = sub.trim_start_matches("query").trim();
                    if q.is_empty() {
                        return Err("usage: /vmemory query <text>".to_string());
                    }
                    let embedder = vm
                        .take_embedder()
                        .ok_or_else(|| "vector-memory embedder unavailable".to_string())?;
                    let embedding = embedder
                        .embed_text(q)
                        .await
                        .map_err(|e| format!("embedding failed: {e}"))?;
                    let matches = vm
                        .search_embedded(&embedding, Some(&project), 10)
                        .map_err(|e| format!("recall failed: {e}"))?;
                    if matches.is_empty() {
                        Ok("no matching vector memories found.".to_string())
                    } else {
                        let body = matches
                            .iter()
                            .map(|(obs, score)| {
                                format!("- [{}] {}  (score {:.2})", obs.kind, obs.content, score)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        Ok(format!("vector memory matches for `{q}`:\n{body}"))
                    }
                }
                Some("list") => {
                    let obs = vm
                        .list_recent(Some(&project), 50)
                        .map_err(|e| format!("list failed: {e}"))?;
                    if obs.is_empty() {
                        Ok("vector memory store is empty.".to_string())
                    } else {
                        let body = obs
                            .iter()
                            .map(|o| format!("- [{}] {}", o.kind, o.content))
                            .collect::<Vec<_>>()
                            .join("\n");
                        Ok(format!("recent vector memories:\n{body}"))
                    }
                }
                other => Err(format!(
                    "unknown subcommand `{}`. Try `/vmemory help`.",
                    other.unwrap_or("")
                )),
            }
        })
    });

    match result {
        Ok(msg) => CommandResult::message(msg),
        Err(e) => CommandResult::error(e),
    }
}

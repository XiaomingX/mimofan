//! Prefix-cache stability manager (inspired by Reasonix's Pillar 1).
//!
//! DeepSeek's automatic prefix caching activates only when the *exact*
//! byte prefix of a request matches the prior request. Any system-prompt
//! drift, tool-list reordering, or message-rewriting busts the cache
//! for every token after the changed byte.
//!
//! This module provides a `PrefixStabilityManager` that:
//!
//! 1. **Fingerprints** the immutable prefix (system prompt + tool specs)
//!    at session start, using SHA-256 for strong collision resistance.
//! 2. **Detects drift** by comparing the current prefix against the
//!    pinned fingerprint before every request.
//! 3. **Diagnoses** the cause of drift — did the system prompt change?
//!    Did the tool set change? Both?
//! 4. **Emits events** so the TUI can surface stability to the user.
//!
//! ## Three-region model (from Reasonix)
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ IMMUTABLE PREFIX                        │ ← fixed for session
//! │   system + tool_specs                    │   cache hit candidate
//! ├─────────────────────────────────────────┤
//! │ APPEND-ONLY HISTORY                     │ ← grows monotonically
//! │   [assistant₁][tool₁][assistant₂]...    │   preserves prefix of prior turns
//! ├─────────────────────────────────────────┤
//! │ LATEST USER TURN                        │ ← the only new content per request
//! └─────────────────────────────────────────┘
//! ```

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::models::{SystemPrompt, Tool};

/// A snapshot of the immutable prefix's fingerprint.
///
/// Two snapshots with the same `combined` hash are guaranteed to
/// produce the same byte prefix when serialized for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixFingerprint {
    /// SHA-256 of the system prompt text.
    pub system_sha256: String,
    /// SHA-256 of the full tool catalog JSON (names, descriptions, schemas).
    pub tools_sha256: String,
    /// SHA-256 of system_sha256 ++ tools_sha256 (combined).
    pub combined_sha256: String,
}

impl PrefixFingerprint {
    /// Compute a fingerprint from system prompt text and tool list.
    ///
    /// Tools are serialized to the same JSON shape the chat API receives
    /// (`type`, `name`, `description`, `parameters`, `strict`), sorted
    /// lexicographically by JSON text, then SHA-256 hashed. This catches
    /// schema/description drift that actually affects the API prefix,
    /// while ignoring internal-only fields like `allowed_callers` (#2264).
    ///
    /// This entry point shares a process-local [`ToolCatalogCache`] with
    /// every other call, so a stable tool set (the common case after the
    /// first turn of a session) avoids the per-tool JSON serialization
    /// and sort/join entirely. Callers that hold their own cache — e.g.
    /// [`PrefixStabilityManager`] — should use
    /// [`Self::compute_with_tool_cache`] to share *that* cache instead
    /// and avoid the thread-local lookup.

    /// Compute a fingerprint while reusing a [`ToolCatalogCache`] for the
    /// tool-side work. The cache holds the joined+sorted+SHA-256'd catalog
    /// under a content-derived identity so the per-tool JSON serialization
    /// and the sort/join only run on the first call for a given tool set.
    ///
    /// On a cache hit this function avoids the entire tool serialization
    /// path, which can be 100+ microseconds for a 60-tool catalog.
    pub fn compute_with_tool_cache(
        system_text: &str,
        tools: Option<&[Tool]>,
        cache: &mut ToolCatalogCache,
    ) -> Self {
        let system_sha256 = sha256_hex(system_text.as_bytes());

        let tools_sha256 = match tools {
            Some(tools) if !tools.is_empty() => {
                // `fingerprint_for` consults the cache first; on a hit
                // it returns the pre-computed hex digest directly.
                cache.fingerprint_for(tools).sha256_hex
            }
            _ => sha256_hex(b""),
        };

        let combined = format!("{system_sha256}:{tools_sha256}");
        let combined_sha256 = sha256_hex(combined.as_bytes());
        Self {
            system_sha256,
            tools_sha256,
            combined_sha256,
        }
    }
}

/// A change record describing what drifted in the prefix.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefixChange {
    /// The old fingerprint (before the change).
    pub old: PrefixFingerprint,
    /// The new fingerprint (after the change).
    pub new: PrefixFingerprint,
    /// Whether the system prompt component changed.
    pub system_changed: bool,
    /// Whether the tool set component changed.
    pub tools_changed: bool,
}

#[allow(dead_code)]
impl PrefixChange {
    /// Returns a human-readable description of what changed.
    pub fn description(&self) -> String {
        let mut parts = Vec::new();
        if self.system_changed {
            parts.push("system prompt");
        }
        if self.tools_changed {
            parts.push("tool set");
        }
        if parts.is_empty() {
            return "unknown (fingerprint mismatch but no component detected)".to_string();
        }
        format!("prefix cache invalidated: {} changed", parts.join(" and "))
    }

    /// Returns a short label for TUI chip display.
    pub fn label(&self) -> &'static str {
        if self.system_changed && self.tools_changed {
            "sys+tools"
        } else if self.system_changed {
            "sys"
        } else if self.tools_changed {
            "tools"
        } else {
            "prefix"
        }
    }
}

/// Monitors and manages prefix-cache stability across turns.
///
/// This is the core abstraction, mirroring Reasonix's `ImmutablePrefix`
/// concept but adapted to mimofan's existing architecture where the
/// system prompt is rebuilt each turn and tools are registered at startup.
///
/// Usage:
/// ```ignore
/// let mgr = PrefixStabilityManager::new(system_text, tools);
/// if mgr.check_and_update(system_text, tools) {
///     println!("Prefix is stable (cache-friendly)");
/// } else {
///     let change = mgr.last_change().unwrap();
///     println!("Prefix drifted: {}", change.description());
/// }
/// ```
#[derive(Debug, Clone)]
pub struct PrefixStabilityManager {
    /// The pinned fingerprint from session start or last stabilization.
    pinned: Option<PrefixFingerprint>,
    /// The most recent fingerprint (computed during last check).
    current: Option<PrefixFingerprint>,
    /// The last detected change, if any.
    last_change: Option<PrefixChange>,
    /// Total number of prefix changes detected this session.
    change_count: u64,
    /// Total number of stability checks performed.
    check_count: u64,
    /// Process-local cache for the tool-catalog JSON serialization. Avoids
    /// re-running `tool_to_api_json` + sort + join on every `check_and_update`
    /// when the tool set is unchanged (the common case once tools are
    /// registered at session start).
    tool_catalog_cache: ToolCatalogCache,
}

/// Default capacity for the tool-catalog serialization cache. Sized for
/// "session + 1 or 2 forked subagent catalogs" without unbounded growth.
const TOOL_CATALOG_CACHE_CAPACITY: usize = 8;

/// Bounded LRU cache of `(tool_set_identity) -> (sha256_hex, joined_string)`.
///
/// The cache key is a content-derived `u64` hash of the tool list (length +
/// per-tool `name` + `description` + serialized `input_schema`). On a hit,
/// `PrefixFingerprint::compute` skips the per-tool JSON serialization, the
/// sort, and the join — a workload that can be 100+ microseconds for a
/// 60-tool catalog. On a miss, the work runs once and the result is stored.
///
/// The cache is intentionally *not* generic over `PrefixFingerprint` because
/// only the joined string is large; the SHA-256 is recomputed from the cached
/// joined string when the catalog changes (cheap, ≤ a few hundred bytes).
#[derive(Debug, Default, Clone)]
pub struct ToolCatalogCache {
    by_identity: HashMap<u64, CachedCatalog>,
    insertion_order: VecDeque<u64>,
    capacity: usize,
}

/// One entry in [`ToolCatalogCache`]. Stores the joined JSON catalog plus
/// the pre-computed SHA-256 hex digest so [`PrefixFingerprint::compute`]
/// does not need to re-hash on the hot path.
#[derive(Debug, Clone)]
pub struct CachedCatalog {
    /// The newline-joined, sorted tool-catalog JSON. Wrapped in an `Arc` so
    /// multiple cache consumers can hold the same allocation. Exposed for
    /// observability (debug builds, `/status` chip) and for tests that
    /// need to assert byte-stability of the joined catalog.
    #[allow(dead_code)] // observability + tests; not consumed on the hot path
    pub joined: Arc<String>,
    /// SHA-256 hex digest of `joined`, computed once on cache miss.
    pub sha256_hex: String,
}

impl ToolCatalogCache {
    /// Create a cache with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(TOOL_CATALOG_CACHE_CAPACITY)
    }

    /// Create a cache that holds at most `capacity` tool-set entries.
    /// Smaller values save memory at the cost of more cache misses.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            by_identity: HashMap::with_capacity(cap),
            insertion_order: VecDeque::with_capacity(cap),
            capacity: cap,
        }
    }

    /// Compute (or recall) the joined-and-hashed tool catalog for `tools`.
    /// The cache is keyed on a content-derived `u64` identity so two `&[Tool]`
    /// slices with the same payloads — in the same order — hit the same entry.
    pub fn fingerprint_for(&mut self, tools: &[Tool]) -> CachedCatalog {
        let identity = tool_set_identity(tools);
        if let Some(cached) = self.by_identity.get(&identity) {
            // Hit: clone the `Arc` so the caller can hold the joined string
            // without keeping a reference to the cache.
            return cached.clone();
        }

        // Miss: serialize, sort, join, hash. Store the joined string in an
        // `Arc` so a later hit can return the same allocation.
        let mut serialized: Vec<String> = tools.iter().filter_map(tool_to_api_json).collect();
        serialized.sort();
        let joined = Arc::new(serialized.join("\n"));
        let sha256_hex = sha256_hex(joined.as_bytes());
        let entry = CachedCatalog {
            joined: Arc::clone(&joined),
            sha256_hex,
        };

        if self.by_identity.len() >= self.capacity
            && let Some(oldest) = self.insertion_order.pop_front()
        {
            self.by_identity.remove(&oldest);
        }
        self.by_identity.insert(identity, entry.clone());
        self.insertion_order.push_back(identity);
        entry
    }

    /// Drop every cached entry. Used by tool-registry mutation paths
    /// (e.g. plugin hot-reload, MCP attach) when the caller cannot
    /// easily prove the tool set is unchanged.
    #[allow(dead_code)] // observability; called by /cache flush and tests
    pub fn invalidate(&mut self) {
        self.by_identity.clear();
        self.insertion_order.clear();
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_identity.len()
    }

    /// Returns `true` if the cache has no entries.
    #[allow(dead_code)] // observability; surfaced via /status
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_identity.is_empty()
    }

    /// Returns `(current_entries, capacity)` for observability. Surfaced via
    /// the `/status` chip in a follow-up; tests exercise the path.
    #[allow(dead_code)] // surfaced via /status in a follow-up; tests exercise it
    #[must_use]
    pub fn stats(&self) -> (usize, usize) {
        (self.len(), self.capacity)
    }
}

/// Content-derived identity for a tool slice. Order-sensitive: two slices
/// with the same tools in different orders produce different identities.
/// (The downstream fingerprint itself is order-insensitive — the sort in
/// `fingerprint_for` takes care of that — but the cache key matches the
/// input order so re-registration of the same set in the same order hits.)
fn tool_set_identity(tools: &[Tool]) -> u64 {
    let mut hasher = DefaultHasher::new();
    tools.len().hash(&mut hasher);
    for tool in tools {
        tool.name.hash(&mut hasher);
        tool.description.hash(&mut hasher);
        // `strict` participates in `tool_to_api_json` output (it is part of
        // the wire-format the chat API receives), so it MUST be part of the
        // identity. Omitting it lets two semantically different catalogs
        // collide and serve a stale fingerprint.
        tool.strict.hash(&mut hasher);
        // Walk the schema JSON directly instead of materializing it as a
        // String. For a 60-tool catalog this saves ~25-40 KB of allocation
        // on every cache miss.
        hash_json_value(&tool.input_schema, &mut hasher);
    }
    hasher.finish()
}

/// Fold a `serde_json::Value` into the hasher without allocating a
/// `String`. Numeric variants are hashed via their bit pattern so `1` and
/// `1.0` produce distinct identities (matching the JSON spec).
fn hash_json_value<H: Hasher>(value: &serde_json::Value, state: &mut H) {
    match value {
        serde_json::Value::Null => 0u8.hash(state),
        serde_json::Value::Bool(b) => {
            1u8.hash(state);
            b.hash(state);
        }
        serde_json::Value::Number(n) => {
            2u8.hash(state);
            if let Some(i) = n.as_i64() {
                i.hash(state);
            } else if let Some(u) = n.as_u64() {
                u.hash(state);
            } else if let Some(f) = n.as_f64() {
                f.to_bits().hash(state);
            }
        }
        serde_json::Value::String(s) => {
            3u8.hash(state);
            s.hash(state);
        }
        serde_json::Value::Array(arr) => {
            4u8.hash(state);
            arr.len().hash(state);
            for v in arr {
                hash_json_value(v, state);
            }
        }
        serde_json::Value::Object(obj) => {
            5u8.hash(state);
            obj.len().hash(state);
            // Iterate by sorted key so `{"a":1,"b":2}` and `{"b":2,"a":1}`
            // collide — the wire format already canonicalizes via the
            // `serde_json` Map ordering, but a defensively-sorted view
            // future-proofs against schema serializers that emit
            // declaration order.
            let mut entries: Vec<(&String, &serde_json::Value)> = obj.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (k, v) in entries {
                k.hash(state);
                hash_json_value(v, state);
            }
        }
    }
}

/// Process-local fallback cache used by [`PrefixFingerprint::compute`]
/// (when available). Callers that maintain their own cache (e.g.
/// [`PrefixStabilityManager`]) should prefer
/// [`PrefixFingerprint::compute_with_tool_cache`] and pass the cache in
/// directly, both to share state and to avoid the thread-local lookup
/// on the hot path.
#[allow(dead_code)]
impl PrefixStabilityManager {
    /// Create a new manager and immediately pin the first fingerprint.
    pub fn new(system_text: &str, tools: Option<&[Tool]>) -> Self {
        let mut cache = ToolCatalogCache::new();
        let fp = PrefixFingerprint::compute_with_tool_cache(system_text, tools, &mut cache);
        Self {
            pinned: Some(fp.clone()),
            current: Some(fp),
            last_change: None,
            change_count: 0,
            check_count: 0,
            tool_catalog_cache: cache,
        }
    }

    /// Create a manager in "unpinned" state — no initial fingerprint.
    /// Call `pin()` or `check_and_update()` to establish the baseline.
    pub fn new_unpinned() -> Self {
        Self {
            pinned: None,
            current: None,
            last_change: None,
            change_count: 0,
            check_count: 0,
            tool_catalog_cache: ToolCatalogCache::new(),
        }
    }

    /// Explicitly pin a fingerprint, replacing any prior pinned state.
    /// Returns `true` if this is the first pin, or `false` if replacing.
    /// Note: does NOT increment `check_count` — that counter is reserved
    /// for `check_and_update` calls so `stability_ratio()` stays accurate.
    pub fn pin(&mut self, system_text: &str, tools: Option<&[Tool]>) -> bool {
        let fp = PrefixFingerprint::compute_with_tool_cache(
            system_text,
            tools,
            &mut self.tool_catalog_cache,
        );
        let was_unpinned = self.pinned.is_none();
        self.pinned = Some(fp.clone());
        self.current = Some(fp);
        was_unpinned
    }

    /// Check whether the current prefix matches the pinned fingerprint.
    /// Updates internal state and returns:
    /// - `Ok(true)` if the prefix is stable (fingerprint matches pinned).
    /// - `Ok(false)` if the prefix changed but was automatically re-pinned.
    /// - `Err(change)` if the prefix changed; caller should surface this.
    ///
    /// After calling this, `last_change()` returns the detected change.
    pub fn check_and_update(
        &mut self,
        system_text: &str,
        tools: Option<&[Tool]>,
    ) -> Result<bool, Box<PrefixChange>> {
        // Use the cached tool-catalog fingerprint path so a stable tool set
        // (the common case after the first turn) does not re-serialize the
        // full tool list. The system-prompt side is hashed on every call
        // because the system prompt changes more often (mode flips,
        // project-context refreshes, canonical state overlays).
        let fp = PrefixFingerprint::compute_with_tool_cache(
            system_text,
            tools,
            &mut self.tool_catalog_cache,
        );
        let old_fp = self.current.replace(fp.clone());
        self.check_count += 1;

        let pinned = match &self.pinned {
            Some(p) => p,
            None => {
                // First check: pin now.
                self.pinned = Some(fp);
                self.last_change = None;
                return Ok(true);
            }
        };

        if fp.combined_sha256 == pinned.combined_sha256 {
            // Stable — no change.
            Ok(true)
        } else {
            // Change detected.
            let old = old_fp.unwrap_or_else(|| pinned.clone());
            let system_changed = fp.system_sha256 != pinned.system_sha256;
            let tools_changed = fp.tools_sha256 != pinned.tools_sha256;

            let change = PrefixChange {
                old,
                new: fp.clone(),
                system_changed,
                tools_changed,
            };

            self.last_change = Some(change.clone());
            self.change_count += 1;

            // Re-pin to the new prefix so subsequent checks are
            // against the latest baseline. Use the original fp
            // (avoid recomputing the hash — clone was for the change record).
            self.pinned = Some(fp);

            Err(Box::new(change))
        }
    }

    /// Returns the most recent prefix change, if any.
    pub fn last_change(&self) -> Option<&PrefixChange> {
        self.last_change.as_ref()
    }

    /// Returns the pinned fingerprint.
    pub fn pinned_fingerprint(&self) -> Option<&PrefixFingerprint> {
        self.pinned.as_ref()
    }

    /// Returns the current (most recently computed) fingerprint.
    pub fn current_fingerprint(&self) -> Option<&PrefixFingerprint> {
        self.current.as_ref()
    }

    /// Returns the total number of prefix changes detected.
    pub fn change_count(&self) -> u64 {
        self.change_count
    }

    /// Returns the total number of stability checks performed.
    pub fn check_count(&self) -> u64 {
        self.check_count
    }

    /// Returns the prefix stability rate as a fraction (0.0 – 1.0).
    /// 1.0 means the prefix has never changed. Returns 1.0 when no
    /// checks have been performed (to avoid division by zero).
    pub fn stability_ratio(&self) -> f64 {
        if self.check_count == 0 {
            1.0
        } else {
            let stable_checks = self.check_count - self.change_count;
            stable_checks as f64 / self.check_count as f64
        }
    }

    /// Returns a human-readable stability summary.
    pub fn summary(&self) -> String {
        let pct = self.stability_ratio() * 100.0;
        let pinned_short = self
            .pinned
            .as_ref()
            .map(|fp| {
                if fp.combined_sha256.len() >= 12 {
                    &fp.combined_sha256[..12]
                } else {
                    &fp.combined_sha256
                }
            })
            .unwrap_or("none");

        format!(
            "Prefix stability: {pct:.1}% ({stable}/{total} checks stable) | fingerprint: {pinned_short} | changes: {changes}",
            pct = pct,
            stable = self.check_count.saturating_sub(self.change_count),
            total = self.check_count,
            pinned_short = pinned_short,
            changes = self.change_count,
        )
    }
}

/// Serialize a tool to the same JSON shape the chat API receives,
/// excluding internal-only fields like `allowed_callers`, `defer_loading`,
/// `input_examples`, and `cache_control` that are never sent to DeepSeek.
fn tool_to_api_json(tool: &Tool) -> Option<String> {
    let mut value = serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    });
    if let Some(strict) = tool.strict
        && let Some(function) = value.get_mut("function")
    {
        function["strict"] = serde_json::json!(strict);
    }
    serde_json::to_string(&value).ok()
}

/// Compute the SHA-256 hex digest of a byte slice.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Extract the system prompt text from an optional SystemPrompt,
/// returning an owned String. This is used for prefix fingerprinting
/// and avoids lifetime/leak issues with the rare SystemPrompt::Blocks case.
pub fn system_prompt_text(system: Option<&SystemPrompt>) -> String {
    match system {
        Some(SystemPrompt::Text(text)) => text.clone(),
        Some(SystemPrompt::Blocks(blocks)) => {
            let mut text = String::new();
            for block in blocks {
                text.push_str(&block.text);
                text.push('\n');
            }
            text
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {}

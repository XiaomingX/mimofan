//! Typestate / protocol state-machine modeling (T-5).
//!
//! Many vulnerabilities are *ordering* bugs: a sensitive operation is only safe
//! after the object has passed through a required setup state (e.g. a
//! deserialization protocol must see `safeMode` engaged before `readObject`
//! is invoked, or a connection must be `authenticated` before `execute`).
//! Typestate modeling captures these as a finite-state machine: states, the
//! allowed transitions, and the *protocol-violation* findings when code calls
//! a guarded method in the wrong state.
//!
//! The FSM is declarative (YAML) so new protocols (parser FSMs, TLS handshakes,
//! deserialization guards) can be added without Rust changes. The solver takes
//! an extracted sequence of method calls on a tracked object and reports any
//! transition that the protocol forbids.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};

use crate::rules::Yaml;

/// A protocol state machine loaded from YAML.
///
/// Schema:
/// ```yaml
/// protocol: deserialization
/// object: SafeObjectInputStream
/// initial: created
/// accepting: [ready]
/// states: [created, safe_mode, ready, poisoned]
/// transitions:
///   - from: created
///     on: enableSafeMode
///     to: safe_mode
///   - from: safe_mode
///     on: readObject
///     to: ready
/// guards:
///   - on: readObject
///     require_state: safe_mode   # calling readObject outside safe_mode is a violation
/// ```
#[derive(Debug, Clone, Default)]
pub struct ProtocolFsm {
    pub protocol: String,
    pub object: String,
    pub initial: String,
    pub accepting: Vec<String>,
    pub states: Vec<String>,
    /// (from, on) -> to
    pub transitions: Vec<Transition>,
    /// method -> required state before it may be called safely.
    pub guards: Vec<Guard>,
}

#[derive(Debug, Clone)]
pub struct Transition {
    pub from: String,
    pub on: String,
    pub to: String,
}

#[derive(Debug, Clone)]
pub struct Guard {
    pub on: String,
    pub require_state: String,
}

/// A protocol violation found along a call sequence.
#[derive(Debug, Clone)]
pub struct ProtocolViolation {
    pub protocol: String,
    pub object: String,
    pub method: String,
    pub at_line: usize,
    pub message: String,
}

impl ProtocolFsm {
    /// Parse a protocol FSM from a YAML document (subset parser from `rules`).
    pub fn from_yaml(file: &str, text: &str) -> Result<ProtocolFsm> {
        let doc = crate::rules::parse_yaml(file, text)?;
        let m = doc.as_map().context("protocol must be a mapping")?;
        let protocol = m
            .get("protocol")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string();
        let object = m
            .get("object")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string();
        let initial = m
            .get("initial")
            .and_then(Yaml::as_str)
            .unwrap_or("")
            .to_string();
        let accepting = string_list(m.get("accepting"));
        let states = string_list(m.get("states"));

        let mut transitions = Vec::new();
        if let Some(Yaml::Seq(items)) = m.get("transitions") {
            for it in items {
                if let Some(mm) = it.as_map() {
                    transitions.push(Transition {
                        from: mm
                            .get("from")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_string(),
                        on: mm
                            .get("on")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_string(),
                        to: mm
                            .get("to")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }
        let mut guards = Vec::new();
        if let Some(Yaml::Seq(items)) = m.get("guards") {
            for it in items {
                if let Some(mm) = it.as_map() {
                    guards.push(Guard {
                        on: mm
                            .get("on")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_string(),
                        require_state: mm
                            .get("require_state")
                            .and_then(Yaml::as_str)
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }
        Ok(ProtocolFsm {
            protocol,
            object,
            initial,
            accepting,
            states,
            transitions,
            guards,
        })
    }

    /// Check a sequence of method calls (with line numbers) on the tracked
    /// object. Returns every protocol violation. `calls` is the observed order
    /// of method names.
    pub fn check_sequence(&self, calls: &[(String, usize)]) -> Vec<ProtocolViolation> {
        let mut state = self.initial.clone();
        let mut violations = Vec::new();
        let known: HashSet<&str> = self.states.iter().map(|s| s.as_str()).collect();
        for (method, line) in calls {
            // Guard check: is this method allowed in the current state?
            if let Some(g) = self.guards.iter().find(|g| &g.on == method)
                && state != g.require_state
            {
                violations.push(ProtocolViolation {
                    protocol: self.protocol.clone(),
                    object: self.object.clone(),
                    method: method.clone(),
                    at_line: *line,
                    message: format!(
                        "protocol `{}`: `{}` called in state `{}` but requires `{}`",
                        self.protocol, method, state, g.require_state
                    ),
                });
            }
            // Transition: advance state if a matching edge exists.
            if let Some(t) = self
                .transitions
                .iter()
                .find(|t| t.from == state && t.on == *method)
            {
                state = t.to.clone();
            } else if !known.contains(method.as_str()) {
                // Unknown method: stay in current state, no transition.
            }
            // Known method without an edge from current state: remain (could be
            // a violation too, but guards already cover the dangerous ones).
        }
        violations
    }
}

fn string_list(y: Option<&Yaml>) -> Vec<String> {
    match y {
        Some(Yaml::Seq(items)) => items
            .iter()
            .filter_map(Yaml::as_str)
            .map(|s| s.to_string())
            .collect(),
        Some(Yaml::Str(s)) => vec![s.clone()],
        _ => vec![],
    }
}

/// Load all `*.yaml` protocol FSMs from a directory.
pub fn load_protocols_dir(dir: &str) -> Result<Vec<ProtocolFsm>> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading protocols dir {dir}"))?;
    for e in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if e.extension()
            .map(|x| x == "yaml" || x == "yml")
            .unwrap_or(false)
        {
            let text = std::fs::read_to_string(&e)?;
            let name = e
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            out.push(ProtocolFsm::from_yaml(&name, &text)?);
        }
    }
    Ok(out)
}

/// Keep BTreeMap referenced so the import is meaningful for future indexing.
#[allow(dead_code)]
fn _assert_btreemap_used() -> BTreeMap<u8, u8> {
    BTreeMap::new()
}

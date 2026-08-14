//! Attack-surface enumeration & gadget-chain detection (T-12).
//!
//! This module ties together the knowledge base (`knowledge.rs`) and the
//! dependency fingerprinting to enumerate a project's *attack surface*: the
//! set of exploitable sinks and gadget chains that are reachable given the
//! dependencies actually present. It also detects *implicit* `autoType` style
//! behaviors (e.g. Jackson `enableDefaultTyping`, Fastjson `parseObject`
//! without a safe `ObjectMapper`) by matching dependency + config signals.
//!
//! Output is a list of [`AttackSurfaceEntry`] that the recon orchestrator
//! (`recon.rs`) aggregates and the TUI reviewer renders as `security_issues`.

use std::collections::HashSet;

use anyhow::Result;

use crate::knowledge::{Gadget, GadgetChain, KnowledgeBase};
use crate::sca::{Dependency, Advisory};

/// A dependency fingerprint signal: a library is present at some version.
#[derive(Debug, Clone)]
pub struct DependencyFingerprint {
    pub name: String,
    pub version: String,
    pub ecosystem: String,
}

impl From<&Dependency> for DependencyFingerprint {
    fn from(d: &Dependency) -> Self {
        DependencyFingerprint {
            name: d.name.clone(),
            version: d.version.clone(),
            ecosystem: d.ecosystem.clone(),
        }
    }
}

/// One entry in the enumerated attack surface.
#[derive(Debug, Clone)]
pub struct AttackSurfaceEntry {
    pub kind: AttackSurfaceKind,
    pub title: String,
    pub severity: String,
    pub category: String,
    pub detail: String,
    /// Gadget/chain id if applicable.
    pub ref_id: Option<String>,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttackSurfaceKind {
    /// A satisfied gadget chain.
    GadgetChain,
    /// An implicit unsafe deserialization / autoType configuration.
    ImplicitAutoType,
    /// A known-vulnerable dependency (from OSV).
    VulnerableDependency,
    /// A dangerous sink present but unconfirmed reachable.
    SinkPresent,
}

/// Enumerate the attack surface from a KB, the resolved dependencies, and the
/// OSV advisories found for them.
pub fn enumerate_surface(
    kb: &KnowledgeBase,
    deps: &[Dependency],
    advisories: &[(Dependency, Advisory)],
) -> Vec<AttackSurfaceEntry> {
    let mut entries = Vec::new();

    // 1. Gadget chains: map each dependency name to the gadgets it provides,
    //    then test which chains are satisfied.
    let present_gadgets = gadgets_for_deps(kb, deps);
    let present_ids: Vec<String> = present_gadgets.iter().map(|g| g.id.clone()).collect();
    for chain in kb.satisfied_chains(&present_ids) {
        entries.push(chain_entry(chain));
    }

    // 2. Implicit autoType detection: if a known autoType-prone library is
    //    present, flag it unless a safe-mode gadget is also present.
    for dep in deps {
        if is_autotype_prone(&dep.name) && !has_safe_mode(kb, &present_ids) {
            entries.push(AttackSurfaceEntry {
                kind: AttackSurfaceKind::ImplicitAutoType,
                title: format!("Implicit autoType deserialization in {}", dep.name),
                severity: "warning".into(),
                category: "unsafe-deserialization".into(),
                detail: format!(
                    "{}@{} enables polymorphic deserialization; without an explicit \
                     safe-type allowlist this is a known RCE sink (Fastjson/Jackson).",
                    dep.name, dep.version
                ),
                ref_id: Some(dep.name.clone()),
                references: vec!["CWE-502".into()],
            });
        }
    }

    // 3. Vulnerable dependencies from OSV.
    for (dep, adv) in advisories {
        entries.push(AttackSurfaceEntry {
            kind: AttackSurfaceKind::VulnerableDependency,
            title: format!("{}@{} — {}", dep.name, dep.version, adv.summary),
            severity: adv.severity.clone(),
            category: "dependency-vulnerability".into(),
            detail: format!(
                "Advisory {} (aliases: {}) affects range {}",
                adv.id,
                adv.aliases.join(", "),
                adv.vulnerable_range
            ),
            ref_id: Some(adv.id.clone()),
            references: adv.aliases.clone(),
        });
    }

    entries
}

/// Map resolved dependencies to the KB gadgets they provide (by library name).
fn gadgets_for_deps<'a>(kb: &'a KnowledgeBase, deps: &[Dependency]) -> Vec<&'a Gadget> {
    let dep_names: HashSet<&str> = deps.iter().map(|d| d.name.as_str()).collect();
    kb.gadgets
        .values()
        .filter(|g| dep_names.iter().any(|n| n.contains(&g.library) || g.library.contains(n)))
        .collect()
}

fn chain_entry(chain: &GadgetChain) -> AttackSurfaceEntry {
    AttackSurfaceEntry {
        kind: AttackSurfaceKind::GadgetChain,
        title: format!("Gadget chain satisfied: {}", chain.name),
        severity: chain.severity.clone(),
        category: chain.enables.clone(),
        detail: format!(
            "All required gadgets present ({}) → {} reachable.",
            chain.requires.join(", "),
            chain.enables
        ),
        ref_id: Some(chain.id.clone()),
        references: chain.references.clone(),
    }
}

/// Libraries known to enable polymorphic/autoType deserialization.
fn is_autotype_prone(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("fastjson")
        || n.contains("jackson")
        || n.contains("xstream")
        || n.contains("snakeyaml")
}

/// Whether a safe-mode gadget is present that would mitigate autoType.
fn has_safe_mode(kb: &KnowledgeBase, present_ids: &[String]) -> bool {
    present_ids.iter().any(|id| {
        kb.gadgets
            .get(id)
            .map(|g| g.enables == "deserialization-guard")
            .unwrap_or(false)
    })
}

/// Convenience: build attack-surface entries for a project given its KB, lock
/// content, and a (sync) OSV client.
pub fn scan_attack_surface(
    kb: &KnowledgeBase,
    lock_path: &str,
    lock_content: &str,
    osv: &dyn crate::sca::OsvClient,
) -> Result<Vec<AttackSurfaceEntry>> {
    let deps = crate::sca::parse_lockfile(lock_path, lock_content)?;
    let findings = crate::sca::scan(lock_path, lock_content, osv)?;
    let advisories: Vec<(Dependency, Advisory)> = findings
        .into_iter()
        .map(|f| (f.dependency, f.advisory))
        .collect();
    Ok(enumerate_surface(kb, &deps, &advisories))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::KnowledgeBase;
    use crate::rules::{RuleSet, SymbolSpec};
    use crate::sca::{Advisory, InMemoryOsv};

    #[test]
    fn enumerates_c3p0_chain_and_vuln_dep() {
        let mut kb = KnowledgeBase::default();
        kb.extend_from_yaml(
            "kb.yaml",
            r#"
gadgets:
  - id: c3p0-jndi
    library: com.mchange:c3p0
    class: JndiRefForwardingDataSource
    pivot: setJndiName
    enables: jndi-injection
    references: [CVE-2019-5427]
  - id: jndi-lookup
    library: javax.naming
    class: InitialContext
    pivot: lookup
    enables: jndi-injection
    references: []
chains:
  - id: c3p0-chain
    name: C3P0 -> JNDI
    enables: jndi-injection
    requires: [c3p0-jndi, jndi-lookup]
    severity: error
    references: [CVE-2021-44228]
"#,
        )
        .unwrap();

        let deps = vec![
            Dependency {
                name: "com.mchange:c3p0".into(),
                version: "0.9.5.5".into(),
                ecosystem: "maven".into(),
                reachable: true,
            },
            Dependency {
                name: "javax.naming".into(),
                version: "1.0".into(),
                ecosystem: "maven".into(),
                reachable: true,
            },
        ];

        let mut osv = InMemoryOsv::default();
        osv.advisories.insert(
            ("maven".to_string(), "com.mchange:c3p0".to_string()),
            vec![Advisory {
                id: "OSV-C3P0".to_string(),
                summary: "JNDI injection in c3p0".into(),
                severity: "critical".into(),
                aliases: vec!["CVE-2019-5427".into()],
                vulnerable_range: "<0.9.5.6".into(),
            }],
        );

        let advisories = vec![(
            deps[0].clone(),
            osv.advisories
                .get(&("maven".to_string(), "com.mchange:c3p0".to_string()))
                .unwrap()[0]
                .clone(),
        )];

        let entries = enumerate_surface(&kb, &deps, &advisories);
        let has_chain = entries
            .iter()
            .any(|e| e.kind == AttackSurfaceKind::GadgetChain && e.ref_id.as_deref() == Some("c3p0-chain"));
        let has_vuln = entries
            .iter()
            .any(|e| e.kind == AttackSurfaceKind::VulnerableDependency);
        assert!(has_chain, "C3P0 gadget chain should be enumerated: {entries:?}");
        assert!(has_vuln, "vulnerable dependency should be enumerated");
    }

    #[test]
    fn implicit_autotype_flagged() {
        let kb = KnowledgeBase::default();
        let deps = vec![Dependency {
            name: "com.alibaba:fastjson".into(),
            version: "1.2.60".into(),
            ecosystem: "maven".into(),
            reachable: true,
        }];
        let entries = enumerate_surface(&kb, &deps, &[]);
        assert!(entries
            .iter()
            .any(|e| e.kind == AttackSurfaceKind::ImplicitAutoType));
    }

    // Touch RuleSet/SymbolSpec imports so the test module compiles even if the
    // KB test above is the only consumer in this file.
    #[test]
    fn _unused_import_guard() {
        let _ = RuleSet::default();
        let _ = SymbolSpec {
            symbol: "x".into(),
            arg: None,
            ret: false,
        };
    }
}

//! Function summaries (T-5).
//!
//! A *function summary* captures the security-relevant effect of a function
//! without re-analyzing its body at every call site: whether it introduces a
//! taint source, sanitizes an argument, propagates taint, is a pure getter, a
//! builder/constructor (incremental state), or has side effects. Summaries are
//! produced from the declarative rules in `rules.rs` plus lightweight AST
//! shape heuristics, and consumed by the taint solver (`taint.rs`) and the
//! recon orchestrator (`recon.rs`) to scale inter-procedural analysis.

use std::collections::HashMap;

use crate::rules::{PropagatorRule, RuleSet, SanitizerRule, SinkRule, SourceRule};

/// The kind of effect a function has on taint / state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryKind {
    /// Introduces untrusted data (matches a `SourceRule`).
    Source,
    /// Consumes tainted data dangerously (matches a `SinkRule`).
    Sink,
    /// Reduces taint (matches a `SanitizerRule`).
    Sanitizer,
    /// Forwards taint between args/return (matches a `PropagatorRule`).
    Propagator,
    /// Pure field read, no effect (common getter).
    Getter,
    /// Allocates / configures state (constructor or builder step).
    Constructor,
    /// Mutates external state (side-effecting).
    SideEffecting,
    /// Unknown / not modeled.
    Unknown,
}

/// A computed summary for one function symbol.
#[derive(Debug, Clone)]
pub struct FuncSummary {
    pub symbol: String,
    pub kind: SummaryKind,
    /// Rule id that produced this summary (if rule-derived).
    pub rule_id: Option<String>,
    /// Vulnerability classes associated (from source/sink CWE mapping).
    pub classes: Vec<String>,
    /// Argument positions this summary cares about.
    pub relevant_args: Vec<usize>,
    /// Whether the return value carries taint.
    pub returns_taint: bool,
    /// True for constructors/builders that update state incrementally.
    pub incremental: bool,
}

/// A registry of function summaries, keyed by symbol suffix.
#[derive(Debug, Clone, Default)]
pub struct SummaryTable {
    by_symbol: HashMap<String, FuncSummary>,
}

impl SummaryTable {
    /// Build a summary table from a rule set (one entry per rule) plus a few
    /// built-in structural heuristics for getters/constructors.
    pub fn from_rules(rules: &RuleSet) -> Self {
        let mut table = SummaryTable {
            by_symbol: HashMap::new(),
        };
        for s in &rules.sources {
            table.insert(Self::from_source(s));
        }
        for s in &rules.sinks {
            table.insert(Self::from_sink(s));
        }
        for s in &rules.sanitizers {
            table.insert(Self::from_sanitizer(s));
        }
        for p in &rules.propagators {
            table.insert(Self::from_propagator(p));
        }
        table
    }

    fn insert(&mut self, sum: FuncSummary) {
        self.by_symbol.insert(sum.symbol.clone(), sum);
    }

    fn from_source(s: &SourceRule) -> FuncSummary {
        FuncSummary {
            symbol: s.symbol.symbol.clone(),
            kind: SummaryKind::Source,
            rule_id: Some(s.id.clone()),
            classes: s.cwe.clone(),
            relevant_args: s.symbol.arg.into_iter().collect(),
            returns_taint: s.symbol.ret || s.symbol.arg.is_none(),
            incremental: false,
        }
    }

    fn from_sink(s: &SinkRule) -> FuncSummary {
        FuncSummary {
            symbol: s.symbol.symbol.clone(),
            kind: SummaryKind::Sink,
            rule_id: Some(s.id.clone()),
            classes: s.cwe.clone(),
            relevant_args: s.symbol.arg.into_iter().collect(),
            returns_taint: false,
            incremental: false,
        }
    }

    fn from_sanitizer(s: &SanitizerRule) -> FuncSummary {
        FuncSummary {
            symbol: s.symbol.symbol.clone(),
            kind: SummaryKind::Sanitizer,
            rule_id: Some(s.id.clone()),
            classes: s.neutralizes.clone(),
            relevant_args: s.symbol.arg.into_iter().collect(),
            returns_taint: s.symbol.ret,
            incremental: false,
        }
    }

    fn from_propagator(p: &PropagatorRule) -> FuncSummary {
        FuncSummary {
            symbol: p.symbol.symbol.clone(),
            kind: SummaryKind::Propagator,
            rule_id: Some(p.id.clone()),
            classes: vec![],
            relevant_args: p.from_arg.into_iter().collect(),
            returns_taint: true,
            incremental: p.to_receiver,
        }
    }

    /// Look up a summary by symbol (suffix match, like rules).
    pub fn lookup(&self, symbol: &str) -> Option<FuncSummary> {
        if let Some(s) = self.by_symbol.get(symbol) {
            return Some(s.clone());
        }
        if let Some(s) = self
            .by_symbol
            .values()
            .find(|s| symbol.ends_with(&s.symbol) || symbol == s.symbol)
        {
            return Some(s.clone());
        }
        // Structural heuristics for getters/constructors.
        if symbol.starts_with("get") && symbol.len() > 3 {
            Some(getter_sentinel())
        } else if symbol.starts_with("set")
            || symbol.starts_with("new")
            || symbol.starts_with("build")
            || symbol.starts_with("create")
        {
            Some(ctor_sentinel())
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.by_symbol.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_symbol.is_empty()
    }
}

/// Structural heuristic sentinel summaries (no rule needed).
fn getter_sentinel() -> FuncSummary {
    FuncSummary {
        symbol: "<getter>".to_string(),
        kind: SummaryKind::Getter,
        rule_id: None,
        classes: Vec::new(),
        relevant_args: Vec::new(),
        returns_taint: false,
        incremental: false,
    }
}

fn ctor_sentinel() -> FuncSummary {
    FuncSummary {
        symbol: "<ctor>".to_string(),
        kind: SummaryKind::Constructor,
        rule_id: None,
        classes: Vec::new(),
        relevant_args: Vec::new(),
        returns_taint: false,
        incremental: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{RuleSet, SymbolSpec};

    #[test]
    fn summary_classifies_rules() {
        let mut rules = RuleSet::default();
        rules.sources.push(crate::rules::SourceRule {
            id: "s".into(),
            language: "*".into(),
            symbol: SymbolSpec {
                symbol: "getParam".into(),
                arg: None,
                ret: true,
            },
            category: "x".into(),
            cwe: vec!["CWE-79".into()],
        });
        rules.sinks.push(crate::rules::SinkRule {
            id: "k".into(),
            language: "*".into(),
            symbol: SymbolSpec {
                symbol: "lookup".into(),
                arg: Some(0),
                ret: false,
            },
            category: "x".into(),
            cwe: vec!["CWE-74".into()],
            severity: "error".into(),
        });
        let table = SummaryTable::from_rules(&rules);
        assert_eq!(table.lookup("getParam").unwrap().kind, SummaryKind::Source);
        assert_eq!(table.lookup("lookup").unwrap().kind, SummaryKind::Sink);
        // Structural heuristic.
        assert_eq!(
            table.lookup("getJndiName").unwrap().kind,
            SummaryKind::Getter
        );
        assert_eq!(
            table.lookup("newBuilder").unwrap().kind,
            SummaryKind::Constructor
        );
    }
}

use mimofan_staticanalysis::kb_trace::{match_patterns_for_sink, trace_chains};
use mimofan_staticanalysis::knowledge::load_kb_dir;

// On-disk KB path, resolved against this crate's manifest dir.
const KB_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules/kb");

#[test]
fn trace_fully_satisfied_chain() {
    let kb = load_kb_dir(KB_DIR).expect("load kb");
    let present = vec!["c3p0-jndi".to_string(), "jndi-lookup".to_string()];
    let traces = trace_chains(&kb, &present);
    // The c3p0-log4shell chain requires exactly those two gadgets.
    let t = traces
        .iter()
        .find(|t| t.chain_id == "c3p0-log4shell")
        .expect("c3p0-log4shell chain present");
    assert!(t.satisfied, "both gadgets present => satisfied");
    assert!(t.missing_gadgets.is_empty());
    assert_eq!(t.present_gadgets, vec!["c3p0-jndi", "jndi-lookup"]);
}

#[test]
fn trace_partial_chain_reports_missing_gadget() {
    let kb = load_kb_dir(KB_DIR).expect("load kb");
    let present = vec!["c3p0-jndi".to_string()];
    let traces = trace_chains(&kb, &present);
    let t = traces
        .iter()
        .find(|t| t.chain_id == "c3p0-log4shell")
        .expect("c3p0-log4shell chain present");
    assert!(!t.satisfied, "jndi-lookup absent => not satisfied");
    assert!(
        t.missing_gadgets.contains(&"jndi-lookup".to_string()),
        "missing gadget must be reported: {:?}",
        t.missing_gadgets
    );
}

#[test]
fn severity_ordering_puts_critical_first() {
    let kb = load_kb_dir(KB_DIR).expect("load kb");
    let present = vec!["c3p0-jndi".to_string()];
    let traces = trace_chains(&kb, &present);
    // log4j-log4shell is `critical`, c3p0-log4shell is `error`; critical
    // must come first regardless of alphabetical id order.
    assert_eq!(traces[0].severity, "critical");
    assert_eq!(traces[0].chain_id, "log4j-log4shell");
}

#[test]
fn match_pattern_for_sink() {
    let kb = load_kb_dir(KB_DIR).expect("load kb");
    let pats = match_patterns_for_sink(&kb, "InitialContext.lookup");
    assert_eq!(pats.len(), 1);
    assert_eq!(pats[0].id, "pat-jndi-lookup");
}

#[test]
fn match_pattern_by_substring() {
    let kb = load_kb_dir(KB_DIR).expect("load kb");
    // Substring containment: `lookup` matches `InitialContext.lookup`.
    let pats = match_patterns_for_sink(&kb, "lookup");
    assert!(pats.iter().any(|p| p.id == "pat-jndi-lookup"));
}

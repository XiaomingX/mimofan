use mimofan_staticanalysis::typestate::{load_protocols_dir, ProtocolFsm};

const DESER: &str = r#"
protocol: deserialization
object: SafeObjectInputStream
initial: created
accepting: [ready]
states: [created, safe_mode, ready, poisoned]
transitions:
  - from: created
    on: enableSafeMode
    to: safe_mode
  - from: safe_mode
    on: readObject
    to: ready
guards:
  - on: readObject
    require_state: safe_mode
"#;

#[test]
fn loads_real_protocol_from_disk() {
    // Prove the shipped protocol FSM data is real, not a stub.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules/protocols");
    let fsms = load_protocols_dir(dir).expect("load protocols dir");
    assert!(
        !fsms.is_empty(),
        "expected at least one protocol FSM on disk"
    );
}

#[test]
fn detects_unsafe_deserialization_order() {
    let fsm = ProtocolFsm::from_yaml("deser.yaml", DESER).unwrap();
    // readObject BEFORE enableSafeMode -> violation.
    let violations = fsm.check_sequence(&[
        ("readObject".to_string(), 12),
        ("enableSafeMode".to_string(), 13),
    ]);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].method, "readObject");
    assert!(violations[0].message.contains("safe_mode"));
}

#[test]
fn safe_order_passes() {
    let fsm = ProtocolFsm::from_yaml("deser.yaml", DESER).unwrap();
    let violations = fsm.check_sequence(&[
        ("enableSafeMode".to_string(), 10),
        ("readObject".to_string(), 11),
    ]);
    assert!(
        violations.is_empty(),
        "safe order should pass: {violations:?}"
    );
}

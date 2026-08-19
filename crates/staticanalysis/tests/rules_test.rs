use mimofan_staticanalysis::rules::{load_rules_dir, parse_yaml, RuleSet, SymbolSpec};
use serde_json::json;

const SAMPLE: &str = r#"
sources:
  - id: java-servlet-request
    language: java
    symbol: getParameter
    category: untrusted-input
    cwe: [CWE-20, CWE-79]
sinks:
  - id: java-jndi-lookup
    language: java
    symbol: InitialContext.lookup
    category: jndi-injection
    cwe: [CWE-74]
    severity: error
sanitizers:
  - id: java-esapi-encode
    language: java
    symbol: ESAPI.encoder.encodeForHTML
    neutralizes: [xss]
propagators:
  - id: java-string-concat
    language: java
    symbol: concat
    from_arg: 0
    to_receiver: false
"#;

#[test]
fn parses_rule_document() {
    let mut set = RuleSet::default();
    set.extend_from_yaml("sample.yaml", SAMPLE).unwrap();
    assert_eq!(set.sources.len(), 1);
    assert_eq!(set.sinks.len(), 1);
    assert_eq!(set.sanitizers.len(), 1);
    assert_eq!(set.propagators.len(), 1);

    let src = &set.sources[0];
    assert_eq!(src.id, "java-servlet-request");
    assert_eq!(src.cwe, vec!["CWE-20", "CWE-79"]);

    let san = &set.sanitizers[0];
    assert_eq!(san.neutralizes, vec!["xss"]); // partial sanitizer

    let prop = &set.propagators[0];
    assert_eq!(prop.from_arg, Some(0));
    assert!(!prop.to_receiver);
}

#[test]
fn symbol_suffix_matching() {
    let spec = SymbolSpec {
        symbol: "lookup".into(),
        arg: None,
        ret: false,
    };
    assert!(spec.matches("javax.naming.InitialContext.lookup", None));
    assert!(spec.matches("InitialContext.lookup", None));
    assert!(!spec.matches("foo.look", None));

    let arg_spec = SymbolSpec {
        symbol: "getParameter".into(),
        arg: Some(0),
        ret: false,
    };
    assert!(arg_spec.matches("getParameter", Some(0)));
    assert!(!arg_spec.matches("getParameter", Some(1)));
}

#[test]
fn loads_real_rule_files_from_disk() {
    // Prove the shipped YAML rule files are not empty shells: they must
    // parse and yield actual rules. Path is relative to the crate root.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/rules");
    let set = load_rules_dir(dir).expect("load rules dir");
    assert!(
        !set.sources.is_empty(),
        "expected sources from on-disk rules"
    );
    assert!(!set.sinks.is_empty(), "expected sinks from on-disk rules");
}

#[test]
fn yaml_subset_handles_nested_and_inline() {
    let y = parse_yaml(
        "t.yaml",
        r#"
a: 1
b: hello
c:
  - x
  - y
d: [p, q, r]
e:
  f: nested
  g: 2
"#,
    )
    .unwrap();
    let j = y.into_json();
    assert_eq!(j["a"], json!(1));
    assert_eq!(j["b"], json!("hello"));
    assert_eq!(j["c"], json!(["x", "y"]));
    assert_eq!(j["d"], json!(["p", "q", "r"]));
    assert_eq!(j["e"]["f"], json!("nested"));
    assert_eq!(j["e"]["g"], json!(2));
}

// These tests exercise the Java auto-gadget discovery engine and require the
// `lang-java` feature (tree-sitter Java grammar + on-disk rule file). Under the
// default feature set they are not compiled; run them with:
//   cargo test -p mimofan-staticanalysis --features lang-java --test auto_gadget_test
#[cfg(feature = "lang-java")]
mod lang_java_tests {
    use mimofan_staticanalysis::auto_gadget::discover_gadgets;

    #[test]
    fn discovers_runtime_exec_chain() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Vuln.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        writeln!(
            fh,
            "public class Vuln {{ public void handle(String cmd) {{ Runtime.getRuntime().exec(cmd); }} }}"
        )
        .unwrap();

        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        // The pivot (Runtime.exec) and the sink (Runtime.exec) are the same
        // symbol here, so a chain must be reported and the sink hit recorded.
        assert!(
            res.sinks_hit.iter().any(|s| s == "runtime-exec-sink"),
            "expected runtime-exec-sink in sinks_hit; got {res:?}"
        );
        assert!(
            !res.chains.is_empty(),
            "expected at least one gadget chain; got {res:?}"
        );
        assert!(
            res.chains
                .iter()
                .any(|c| c.pivot_id == "runtime-exec" && c.sink_id == "runtime-exec-sink"),
            "expected a runtime-exec pivot->sink chain; chains={:?}",
            res.chains
        );
    }

    #[test]
    fn discovers_jndi_chain_across_calls() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Svc.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        // pivot-bearing method calls a sink-bearing method transitively.
        writeln!(
            fh,
            "public class Svc {{ \
               public void outer() {{ inner(); }} \
               public void inner() {{ javax.naming.InitialContext ctx = null; ctx.lookup(\"ldap://x\"); }} \
             }}"
        )
        .unwrap();

        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        assert!(
            res.sinks_hit.iter().any(|s| s == "jndi-lookup-sink"),
            "expected jndi-lookup-sink; got {res:?}"
        );
        assert!(
            res.chains
                .iter()
                .any(|c| c.pivot_id == "jndi-lookup" && c.sink_id == "jndi-lookup-sink"),
            "expected jndi-lookup pivot->sink chain; chains={:?}",
            res.chains
        );
    }

    #[test]
    fn no_chain_for_benign_code() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("Benign.java");
        let mut fh = std::fs::File::create(&f).unwrap();
        // A perfectly ordinary method with no dangerous pivot/sink symbol.
        writeln!(
            fh,
            "public class Benign {{ public void only() {{ java.io.File f = new java.io.File(\"x\"); f.listFiles(); }} }}"
        )
        .unwrap();
        let res = discover_gadgets(tmp.path()).expect("discovery ok");
        // No dangerous symbol => no pivot observed, no chain formed.
        assert!(
            res.pivots_observed.is_empty(),
            "benign code must not observe any pivot; got {:?}",
            res.pivots_observed
        );
        assert!(
            res.chains.is_empty(),
            "benign code must not yield gadget chains; chains={:?}",
            res.chains
        );
    }
}

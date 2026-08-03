// Tests relocated from src/cli_commands/metrics.rs (issue #547 Phase 3).

    use mimofan::cli_commands::metrics::*;
    use chrono::{DateTime, Duration, Utc};
    use std::path::Path;

    // ── Duration parser ──

    #[test]
    fn parse_since_7d() {
        let cutoff = parse_since("7d").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::days(7);
        // Allow ±2s for test execution time.
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_24h() {
        let cutoff = parse_since("24h").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::hours(24);
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_30m() {
        let cutoff = parse_since("30m").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::minutes(30);
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_now_prefix() {
        // "now-2h" should strip "now-" and parse "2h".
        let cutoff = parse_since("now-2h").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::hours(2);
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_compound() {
        let cutoff = parse_since("2h30m").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::seconds(2 * 3600 + 30 * 60);
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_compound_days_hours() {
        let cutoff = parse_since("1d12h").expect("parse_since must succeed");
        let expected = Utc::now() - Duration::seconds(36 * 3600);
        assert!((cutoff - expected).num_seconds().abs() < 2);
    }

    #[test]
    fn parse_since_error_on_invalid() {
        assert!(parse_since("xyz").is_err());
        assert!(parse_since("").is_err());
    }

    // ── fmt_num ──

    #[test]
    fn fmt_num_zero() {
        assert_eq!(fmt_num(0), "0");
    }

    #[test]
    fn fmt_num_thousands() {
        assert_eq!(fmt_num(1_000), "1,000");
        assert_eq!(fmt_num(12_453), "12,453");
        assert_eq!(fmt_num(1_000_000), "1,000,000");
    }

    // ── Rollup from audit log ──

    fn make_audit_line(event: &str, tool: &str, ts: &str) -> String {
        format!(
            r#"{{"details":{{"mode":"YOLO","session_id":null,"tool_name":"{tool}"}},"event":"{event}","ts":"{ts}"}}"#
        )
    }

    #[test]
    fn audit_log_empty_file() {
        let mut rollup = Rollup::default();
        // Non-existent path — should not panic, rollup stays empty.
        read_audit_log(Path::new("/nonexistent/audit.log"), None, &mut rollup);
        assert_eq!(rollup.total_lines, 0);
    }

    #[test]
    fn audit_log_parses_auto_approve() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().expect("create named temp file");
        let line1 = make_audit_line(
            "tool.approval.auto_approve",
            "exec_shell",
            "2026-04-01T10:00:00+00:00",
        );
        let line2 = make_audit_line(
            "tool.approval.auto_approve",
            "read_file",
            "2026-04-02T10:00:00+00:00",
        );
        writeln!(tmp, "{line1}").expect("write temp audit file");
        writeln!(tmp, "{line2}").expect("write temp audit file");

        let mut rollup = Rollup::default();
        read_audit_log(tmp.path(), None, &mut rollup);

        assert_eq!(rollup.parsed_lines, 2);
        assert_eq!(rollup.tools["exec_shell"].calls, 1);
        assert_eq!(rollup.tools["exec_shell"].auto_approved, 1);
        assert_eq!(rollup.tools["read_file"].calls, 1);
    }

    #[test]
    fn audit_log_skips_malformed_lines() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().expect("create named temp file");
        writeln!(tmp, "not json at all").expect("write temp audit file");
        writeln!(
            tmp,
            r#"{{"event":"credential.save","ts":"2026-04-01T10:00:00+00:00"}}"#
        )
        .expect("write temp audit file");

        let mut rollup = Rollup::default();
        read_audit_log(tmp.path(), None, &mut rollup);

        // 2 lines total, 1 malformed skipped, 1 parsed.
        assert_eq!(rollup.total_lines, 2);
        assert_eq!(rollup.parsed_lines, 1);
        assert_eq!(rollup.credentials.saves, 1);
    }

    #[test]
    fn audit_log_since_filter() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().expect("create named temp file");
        let line_old = make_audit_line(
            "tool.approval.auto_approve",
            "exec_shell",
            "2025-01-01T00:00:00+00:00",
        );
        let line_new = make_audit_line(
            "tool.approval.auto_approve",
            "read_file",
            "2026-04-01T00:00:00+00:00",
        );
        writeln!(tmp, "{line_old}").expect("write temp audit file");
        writeln!(tmp, "{line_new}").expect("write temp audit file");

        let cutoff: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().expect("parse string");
        let mut rollup = Rollup::default();
        read_audit_log(tmp.path(), Some(cutoff), &mut rollup);

        // Only the newer line should be counted.
        assert_eq!(rollup.parsed_lines, 1);
        assert!(!rollup.tools.contains_key("exec_shell"));
        assert_eq!(rollup.tools["read_file"].calls, 1);
    }

    #[test]
    fn total_tool_calls_sums_across_tools() {
        let mut rollup = Rollup::default();
        rollup.tool_mut("read_file").calls = 4_012;
        rollup.tool_mut("exec_shell").calls = 1_118;
        assert_eq!(rollup.total_tool_calls(), 5_130);
    }

// Tests relocated from src/commands/groups/core/schedule.rs (issue #547 Phase 3).

    use mimofan::commands::groups::core::schedule::*;
    
    
    
    use mimofan::commands::groups::core::schedule::truncate_preview;

    #[test]
    fn test_parse_night_args_valid() {
        let result = parse_night_args("run tests --schedule 00:30");
        assert!(result.is_ok());
        let (prompt, (hour, minute)) = result.expect("unexpected None/Err in test");
        assert_eq!(prompt, "run tests");
        assert_eq!(hour, 0);
        assert_eq!(minute, 30);
    }

    #[test]
    fn test_parse_night_args_missing_schedule() {
        let result = parse_night_args("run tests");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_night_args_invalid_time() {
        let result = parse_night_args("run tests --schedule abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_truncate_preview_short() {
        let text = "short text";
        assert_eq!(truncate_preview(text, 20), "short text");
    }

    #[test]
    fn test_truncate_preview_long() {
        let text = "this is a very long text that should be truncated";
        let result = truncate_preview(text, 20);
        assert!(result.len() <= 23); // 20 + "..."
        assert!(result.ends_with("..."));
    }

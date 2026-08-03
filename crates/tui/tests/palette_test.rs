// Tests relocated from src/palette.rs (issue #547 Phase 3).

    use mimofan::palette::*;

    #[test]
    fn test_theme_names_roundtrip() {
        for theme in SELECTABLE_THEMES {
            let name = theme.name();
            let parsed = ThemeId::from_name(name);
            assert_eq!(parsed, Some(*theme), "Failed roundtrip for {}", name);
        }
    }

    #[test]
    fn test_normalize_theme_aliases() {
        assert_eq!(normalize_theme_name("dark"), Some("dark"));
        assert_eq!(normalize_theme_name("mimofan"), Some("dark"));
        assert_eq!(normalize_theme_name("light"), Some("light"));
        assert_eq!(normalize_theme_name("mimofan-light"), Some("light"));
        assert_eq!(normalize_theme_name("cosmic"), Some("cosmic"));
        assert_eq!(normalize_theme_name("neon"), Some("cosmic"));
        assert_eq!(normalize_theme_name("handwritten"), Some("handwritten"));
        assert_eq!(normalize_theme_name("paper"), Some("handwritten"));
        assert_eq!(normalize_theme_name("crush"), Some("crush"));
        assert_eq!(normalize_theme_name("berry"), Some("crush"));
    }

    #[test]
    fn test_parse_hex_rgb_color() {
        assert!(parse_hex_rgb_color("#ff0000").is_some());
        assert!(parse_hex_rgb_color("00ff00").is_some());
        assert!(parse_hex_rgb_color("#fff").is_none());
        assert!(parse_hex_rgb_color("invalid").is_none());
    }

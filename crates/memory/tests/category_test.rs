use mimofan_memory::category::MemoryCategory;

#[test]
fn parsing_roundtrip() {
    assert_eq!(
        MemoryCategory::from_str_name("user"),
        Some(MemoryCategory::User)
    );
    assert_eq!(
        MemoryCategory::from_str_name("FEEDBACK"),
        Some(MemoryCategory::Feedback)
    );
    assert_eq!(
        MemoryCategory::from_str_name("Project"),
        Some(MemoryCategory::Project)
    );
    assert_eq!(
        MemoryCategory::from_str_name("reference"),
        Some(MemoryCategory::Reference)
    );
    assert_eq!(MemoryCategory::from_str_name("bogus"), None);
    assert_eq!(MemoryCategory::from_str_name(""), None);
    for cat in MemoryCategory::ALL {
        assert_eq!(MemoryCategory::from_str_name(cat.as_str()), Some(*cat));
        assert_eq!(cat.as_str(), cat.as_str().to_ascii_lowercase().as_str());
    }
}

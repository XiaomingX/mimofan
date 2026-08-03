// Tests relocated from src/tools/subagent/custom_agents.rs (issue #547 Phase 3).

use mimofan::tools::subagent::custom_agents::*;
use std::fs;
use std::io::Write;

#[test]
fn test_parse_agent_file_with_frontmatter() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let file_path = dir.path().join("react-expert.md");
    let mut file = fs::File::create(&file_path).expect("create temp file");
    writeln!(
            file,
            "---\nname: react-expert\n description: React specialist\ntools: read_file, write_file\nmodel: fast\n---\n\nYou are a React expert."
        )
        .expect("unexpected None/Err in test");

    let agent = parse_agent_file(&file_path)
        .expect("parse agent file")
        .expect("parse agent file");
    assert_eq!(agent.name, "react-expert");
    assert_eq!(agent.description, "React specialist");
    assert_eq!(agent.tools, vec!["read_file", "write_file"]);
    assert_eq!(agent.model, "fast");
    assert_eq!(agent.prompt, "You are a React expert.");
}

#[test]
fn test_parse_agent_file_without_frontmatter() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let file_path = dir.path().join("simple.md");
    let mut file = fs::File::create(&file_path).expect("create temp file");
    writeln!(file, "You are a helpful assistant.").expect("write agent file");

    let agent = parse_agent_file(&file_path)
        .expect("parse agent file")
        .expect("parse agent file");
    assert_eq!(agent.name, "simple");
    assert!(agent.tools.is_empty());
    assert_eq!(agent.model, "inherit");
    assert_eq!(agent.prompt, "You are a helpful assistant.");
}

#[test]
fn test_registry_scans_directory() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let agents_dir = dir.path().join("agents");
    fs::create_dir(&agents_dir).expect("create temp dir");

    let file_path = agents_dir.join("test-agent.md");
    let mut file = fs::File::create(&file_path).expect("create temp file");
    writeln!(file, "---\ndescription: Test agent\n---\n\nTest prompt.").expect("write agent file");

    let mut registry = CustomAgentRegistry::default();
    CustomAgentRegistry::scan_dir(&agents_dir, &mut registry.agents);

    assert!(registry.contains("test-agent"));
    let agent = registry.get("test-agent").expect("lookup custom agent");
    assert_eq!(agent.description, "Test agent");
}

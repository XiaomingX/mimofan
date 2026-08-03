// Tests relocated from src/tools/subagent/decomposer.rs (issue #547 Phase 3).

use mimofan::tools::subagent::SubAgentType;
use mimofan::tools::subagent::decomposer::*;

fn make_node(id: &str, deps: Vec<&str>) -> TaskNode {
    TaskNode {
        id: id.to_string(),
        description: format!("task {id}"),
        agent_type: SubAgentType::General,
        dependencies: deps.into_iter().map(String::from).collect(),
        status: TaskNodeStatus::Pending,
        result: None,
    }
}

#[test]
fn test_topological_sort_linear_chain() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("b", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("c", vec!["b"]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let order = graph.topological_sort().expect("topological sort");
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn test_topological_sort_diamond() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("b", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("c", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("d", vec!["b", "c"]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let order = graph.topological_sort().expect("topological sort");
    assert_eq!(order[0], "a");
    assert_eq!(order[3], "d");
    // b and c must appear before d.
    let pos_b = order
        .iter()
        .position(|x| x == "b")
        .expect("find position in iterator");
    let pos_c = order
        .iter()
        .position(|x| x == "c")
        .expect("find position in iterator");
    let pos_d = order
        .iter()
        .position(|x| x == "d")
        .expect("find position in iterator");
    assert!(pos_b < pos_d);
    assert!(pos_c < pos_d);
}

#[test]
fn test_topological_sort_cycle_detection() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec!["c"]))
        .expect("add task node");
    graph
        .add_node(make_node("b", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("c", vec!["b"]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let result = graph.topological_sort();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DecomposerError::CycleDetected(_)
    ));
}

#[test]
fn test_parallel_groups_diamond() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("b", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("c", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("d", vec!["b", "c"]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let groups = graph.parallel_groups().expect("compute parallel groups");
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0], vec!["a"]);
    assert!(groups[1].contains(&"b".to_string()));
    assert!(groups[1].contains(&"c".to_string()));
    assert_eq!(groups[2], vec!["d"]);
}

#[test]
fn test_parallel_groups_independent_nodes() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("x", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("y", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("z", vec![]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let groups = graph.parallel_groups().expect("compute parallel groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].len(), 3);
}

#[test]
fn test_ready_nodes() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec![]))
        .expect("add task node");
    graph
        .add_node(make_node("b", vec!["a"]))
        .expect("add task node");
    graph
        .add_node(make_node("c", vec!["a"]))
        .expect("add task node");
    graph.build_edges().expect("build task graph edges");

    let ready = graph.ready_nodes();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, "a");

    graph.start_node("a").expect("start task node");
    graph
        .complete_node("a", Some("done".into()))
        .expect("complete task node");

    let ready = graph.ready_nodes();
    assert_eq!(ready.len(), 2);
}

#[test]
fn test_duplicate_node_rejected() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec![]))
        .expect("add task node");
    let result = graph.add_node(make_node("a", vec![]));
    assert!(matches!(result, Err(DecomposerError::DuplicateNodeId(_))));
}

#[test]
fn test_missing_dependency_rejected() {
    let mut graph = TaskGraph::new();
    graph
        .add_node(make_node("a", vec!["nonexistent"]))
        .expect("add task node");
    let result = graph.build_edges();
    assert!(matches!(
        result,
        Err(DecomposerError::MissingDependency { .. })
    ));
}

#[test]
fn test_decomposer_basic() {
    let decomposer = TaskDecomposer::new();
    let graph = decomposer
        .decompose(vec![
            (
                "step1".into(),
                "explore".into(),
                SubAgentType::Explore,
                vec![],
            ),
            (
                "step2".into(),
                "implement".into(),
                SubAgentType::Implementer,
                vec!["step1".into()],
            ),
            (
                "step3".into(),
                "verify".into(),
                SubAgentType::Verifier,
                vec!["step2".into()],
            ),
        ])
        .expect("unexpected None/Err in test");

    assert_eq!(graph.node_count(), 3);
    let groups = graph.parallel_groups().expect("compute parallel groups");
    assert_eq!(groups.len(), 3);
}

#[test]
fn test_decomposer_cycle_rejected() {
    let decomposer = TaskDecomposer::new();
    let result = decomposer.decompose(vec![
        (
            "a".into(),
            "a".into(),
            SubAgentType::General,
            vec!["b".into()],
        ),
        (
            "b".into(),
            "b".into(),
            SubAgentType::General,
            vec!["a".into()],
        ),
    ]);
    assert!(result.is_err());
}

// Tests relocated from src/fleet/observability.rs (issue #547 Phase 3).

    use mimofan::fleet::observability::*;

    #[test]
    fn test_topology_basics() {
        let mut topo = AgentTopology::new();
        topo.register("root".into(), "child1".into());
        topo.register("root".into(), "child2".into());
        topo.register("child1".into(), "grandchild".into());

        assert_eq!(topo.total_agents(), 4);
        assert_eq!(topo.depth("root"), 2);
        assert_eq!(topo.depth("child1"), 1);
        assert_eq!(topo.depth("grandchild"), 0);
        assert_eq!(topo.roots().len(), 1);
        assert_eq!(topo.children("root").len(), 2);
    }

    #[test]
    fn test_metrics_aggregation() {
        let mut collector = ObservabilityCollector::new();
        collector.record_task_start("task1");
        collector.record_task_completion("agent1", "task1", true, Some(128), Some(45.0));
        collector.record_task_start("task2");
        collector.record_task_completion("agent1", "task2", false, Some(256), None);

        let summary = collector.summary();
        assert_eq!(summary.total_agents, 1);
        assert_eq!(summary.completed_tasks, 1);
        assert_eq!(summary.failed_tasks, 1);
    }

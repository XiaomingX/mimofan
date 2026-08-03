// Tests relocated from src/tools/subagent/bus.rs (issue #547 Phase 3).

    use mimofan::tools::subagent::bus::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = AgentBus::new();
        let (mut rx, _sub) = bus.subscribe("test_topic", "agent_a").await;

        let msg = BusMessage {
            from: "agent_b".into(),
            to: None,
            topic: "test_topic".into(),
            payload: json!({"hello": "world"}),
        };
        bus.publish(msg.clone()).await;

        let received = rx.recv().await.expect("should receive message");
        assert_eq!(received.from, "agent_b");
        assert_eq!(received.topic, "test_topic");
        assert_eq!(received.payload, json!({"hello": "world"}));
    }

    #[tokio::test]
    async fn test_shared_state() {
        let bus = AgentBus::new();
        bus.state_set("key1", json!("value1")).await;
        assert_eq!(bus.state_get("key1").await, Some(json!("value1")));

        let old = bus.state_remove("key1").await;
        assert_eq!(old, Some(json!("value1")));
        assert!(bus.state_get("key1").await.is_none());
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let bus = AgentBus::new();
        let (mut rx1, _sub1) = bus.subscribe("chat", "agent_a").await;
        let (mut rx2, _sub2) = bus.subscribe("chat", "agent_b").await;

        let msg = BusMessage {
            from: "agent_c".into(),
            to: None,
            topic: "chat".into(),
            payload: json!("hi"),
        };
        bus.publish(msg).await;

        assert!(rx1.recv().await.is_some());
        assert!(rx2.recv().await.is_some());
    }

    #[tokio::test]
    async fn test_unrelated_topic_not_delivered() {
        let bus = AgentBus::new();
        let (mut rx, _sub) = bus.subscribe("topic_a", "agent_a").await;

        let msg = BusMessage {
            from: "agent_b".into(),
            to: None,
            topic: "topic_b".into(),
            payload: json!("nope"),
        };
        bus.publish(msg).await;

        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_state_keys() {
        let bus = AgentBus::new();
        bus.state_set("a", json!(1)).await;
        bus.state_set("b", json!(2)).await;
        let mut keys = bus.state_keys().await;
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

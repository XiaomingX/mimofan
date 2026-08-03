use mimofan_mcp::*;
use serde_json::json;
use std::collections::HashMap;

// ── InMemoryMcpClient ──────────────────────────────────────────────

#[test]
fn in_memory_client_list_tools_returns_registered() {
    let client = InMemoryMcpClient::default()
        .with_tool("echo", json!({"output": "hi"}))
        .with_tool("greet", json!({"msg": "hello"}));
    let tools = client
        .list_tools()
        .expect("in_memory_client_list_tools_returns_registered");
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools.iter().map(|t| t.tool_name.as_str()).collect();
    assert!(names.contains(&"echo"));
    assert!(names.contains(&"greet"));
}

#[test]
fn in_memory_client_call_tool_returns_value() {
    let client = InMemoryMcpClient::default().with_tool("echo", json!({"output": "hi"}));
    let result = client
        .call_tool("echo", json!({}))
        .expect("in_memory_client_call_tool_returns_value");
    assert_eq!(result["output"], "hi");
}

#[test]
fn in_memory_client_call_tool_errors_on_missing() {
    let client = InMemoryMcpClient::default();
    let err = client.call_tool("nope", json!({})).unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn in_memory_client_list_resources_returns_registered() {
    let client = InMemoryMcpClient::default()
        .with_resource("mcp://s/health", json!({"ok": true}))
        .with_resource("mcp://s/caps", json!({"tools": []}));
    let resources = client
        .list_resources()
        .expect("in_memory_client_list_resources_returns_registered");
    assert_eq!(resources.len(), 2);
}

#[test]
fn in_memory_client_read_resource_returns_value() {
    let client = InMemoryMcpClient::default().with_resource("mcp://s/health", json!({"ok": true}));
    let result = client
        .read_resource("mcp://s/health")
        .expect("in_memory_client_read_resource_returns_value");
    assert_eq!(result["ok"], true);
}

#[test]
fn in_memory_client_read_resource_errors_on_missing() {
    let client = InMemoryMcpClient::default();
    let err = client.read_resource("mcp://s/nope").unwrap_err();
    assert!(err.to_string().contains("not found"));
}

// ── McpManager ─────────────────────────────────────────────────────

fn make_server_config(name: &str) -> McpServerConfig {
    McpServerConfig {
        name: name.to_string(),
        command: "test".to_string(),
        args: vec![],
        env: HashMap::new(),
        enabled: true,
    }
}

#[test]
fn manager_start_all_marks_ready_for_registered_clients() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default().with_tool("t", json!(null))),
    );
    let mut events = Vec::new();
    let summary = manager.start_all(|e| events.push(e));
    assert_eq!(summary.ready, vec!["s1"]);
    assert!(summary.failed.is_empty());
    assert!(
        events.iter().any(|event| {
            event.server_name == "s1" && event.status == McpStartupStatus::Starting
        })
    );
    assert!(
        events
            .iter()
            .any(|event| { event.server_name == "s1" && event.status == McpStartupStatus::Ready })
    );
}

#[test]
fn manager_start_all_marks_failed_when_client_missing() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default()),
    );
    manager
        .stop_server("s1")
        .expect("manager_start_all_marks_failed_when_client_missing");
    let summary = manager.start_all(|_| {});
    assert!(summary.ready.is_empty());
    assert_eq!(summary.failed.len(), 1);
    assert_eq!(summary.failed[0].server_name, "s1");
}

#[test]
fn manager_start_all_cancels_disabled_servers() {
    let mut manager = McpManager::default();
    let mut cfg = make_server_config("s1");
    cfg.enabled = false;
    manager.register_server(
        cfg,
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default()),
    );
    let summary = manager.start_all(|_| {});
    assert!(summary.ready.is_empty());
    assert_eq!(summary.cancelled, vec!["s1"]);
}

#[test]
fn manager_list_tools_applies_filter() {
    let mut manager = McpManager::default();
    let client = InMemoryMcpClient::default()
        .with_tool("allowed", json!(null))
        .with_tool("denied", json!(null));
    manager.register_server(
        make_server_config("s1"),
        ToolFilter {
            allow: vec!["allowed".to_string()],
            deny: vec![],
        },
        Box::new(client),
    );
    let tools = manager
        .list_tools()
        .expect("manager_list_tools_applies_filter");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "allowed");
}

#[test]
fn manager_list_tools_deny_overrides_allow() {
    let mut manager = McpManager::default();
    let client = InMemoryMcpClient::default()
        .with_tool("a", json!(null))
        .with_tool("b", json!(null));
    manager.register_server(
        make_server_config("s1"),
        ToolFilter {
            allow: vec!["a".to_string(), "b".to_string()],
            deny: vec!["b".to_string()],
        },
        Box::new(client),
    );
    let tools = manager
        .list_tools()
        .expect("manager_list_tools_deny_overrides_allow");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "a");
}

#[test]
fn manager_call_tool_delegates_to_client() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default().with_tool("t", json!({"v": 42}))),
    );
    let result = manager
        .call_tool("s1", "t", json!({}))
        .expect("manager_call_tool_delegates_to_client");
    assert_eq!(result["v"], 42);
}

#[test]
fn manager_call_tool_errors_on_missing_server() {
    let manager = McpManager::default();
    let err = manager.call_tool("nope", "t", json!({})).unwrap_err();
    assert!(err.to_string().contains("not available"));
}

#[test]
fn manager_call_qualified_tool_parses_name() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("my_server"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default().with_tool("my_tool", json!({"ok": true}))),
    );
    let result = manager
        .call_qualified_tool("mcp__my_server__my_tool", json!({}))
        .expect("manager_call_qualified_tool_parses_name");
    assert_eq!(result["ok"], true);
}

#[test]
fn manager_call_qualified_tool_handles_truncated_names() {
    let long_server = "server".repeat(20);
    let long_tool = "tool".repeat(20);
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config(&long_server),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default().with_tool(&long_tool, json!({"ok": true}))),
    );
    let tools = manager
        .list_tools()
        .expect("manager_call_qualified_tool_handles_truncated_names");
    let qualified = &tools[0].qualified_name;
    assert!(qualified.len() <= 64);
    assert!(parse_qualified_tool_name(qualified).is_ok());

    let result = manager
        .call_qualified_tool(qualified, json!({}))
        .expect("manager_call_qualified_tool_handles_truncated_names");
    assert_eq!(result["ok"], true);
}

#[test]
fn manager_unregister_removes_server() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default()),
    );
    manager
        .unregister_server("s1")
        .expect("manager_unregister_removes_server");
    assert!(manager.configs.is_empty());
}

#[test]
fn manager_unregister_errors_on_unknown() {
    let mut manager = McpManager::default();
    let err = manager.unregister_server("nope").unwrap_err();
    assert!(err.to_string().contains("not registered"));
}

#[test]
fn manager_stop_server_errors_on_unknown() {
    let mut manager = McpManager::default();
    let err = manager.stop_server("nope").unwrap_err();
    assert!(err.to_string().contains("not running"));
}

#[test]
fn manager_list_resources_returns_from_clients() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(
            InMemoryMcpClient::default().with_resource("mcp://s1/health", json!({"ok": true})),
        ),
    );
    let resources = manager
        .list_resources()
        .expect("manager_list_resources_returns_from_clients");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].server_name, "s1");
}

#[test]
fn manager_read_resource_delegates() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(
            InMemoryMcpClient::default().with_resource("mcp://s1/health", json!({"ok": true})),
        ),
    );
    let result = manager
        .read_resource("s1", "mcp://s1/health")
        .expect("manager_read_resource_delegates");
    assert_eq!(result["ok"], true);
}

#[test]
fn manager_update_sandbox_state_returns_notices() {
    let mut manager = McpManager::default();
    manager.register_server(
        make_server_config("s1"),
        ToolFilter::default(),
        Box::new(InMemoryMcpClient::default()),
    );
    let notices = manager
        .update_sandbox_state("strict", "/tmp")
        .expect("manager_update_sandbox_state_returns_notices");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0]["server_name"], "s1");
}

// ── Tool filter ────────────────────────────────────────────────────

#[test]
fn allowed_by_filter_empty_allow_permits_all() {
    let filter = ToolFilter {
        allow: vec![],
        deny: vec![],
    };
    assert!(allowed_by_filter("anything", &filter));
}

#[test]
fn allowed_by_filter_deny_blocks() {
    let filter = ToolFilter {
        allow: vec![],
        deny: vec!["danger".to_string()],
    };
    assert!(!allowed_by_filter("danger", &filter));
    assert!(allowed_by_filter("safe", &filter));
}

#[test]
fn allowed_by_filter_allow_only_permits_listed() {
    let filter = ToolFilter {
        allow: vec!["a".to_string()],
        deny: vec![],
    };
    assert!(allowed_by_filter("a", &filter));
    assert!(!allowed_by_filter("b", &filter));
}

// ── Helper functions ───────────────────────────────────────────────

#[test]
fn sanitize_component_lowercases_and_replaces_specials() {
    assert_eq!(sanitize_component("My-Server.Name"), "my_server_name");
    assert_eq!(sanitize_component("ABC123"), "abc123");
}

#[test]
fn qualify_tool_name_produces_mcp_prefix() {
    let name = qualify_tool_name("server", "tool");
    assert!(name.starts_with("mcp__server__tool"));
}

#[test]
fn qualify_tool_name_truncates_long_names() {
    let long_server = "a".repeat(100);
    let name = qualify_tool_name(&long_server, "tool");
    assert!(name.len() <= 64);
    assert!(parse_qualified_tool_name(&name).is_ok());
}

#[test]
fn parse_qualified_tool_name_round_trip() {
    let qualified = qualify_tool_name("my_server", "my_tool");
    let (server, tool) =
        parse_qualified_tool_name(&qualified).expect("parse_qualified_tool_name_round_trip");
    assert_eq!(server, "my_server");
    assert_eq!(tool, "my_tool");
}

#[test]
fn parse_qualified_tool_name_rejects_missing_prefix() {
    let err = parse_qualified_tool_name("not_mcp__server__tool").unwrap_err();
    assert!(err.to_string().contains("missing mcp__ prefix"));
}

#[test]
fn parse_qualified_tool_name_rejects_empty_segments() {
    let err = parse_qualified_tool_name("mcp____tool").unwrap_err();
    assert!(err.to_string().contains("missing server segment"));
}

#[test]
fn parse_server_from_uri_extracts_server() {
    assert_eq!(
        parse_server_from_uri("mcp://my-server/capabilities"),
        Some("my-server".to_string())
    );
}

#[test]
fn parse_server_from_uri_returns_none_for_invalid() {
    assert!(parse_server_from_uri("http://not-mcp").is_none());
    assert!(parse_server_from_uri("mcp:///path").is_none());
}

// ── JsonRpcError ───────────────────────────────────────────────────

#[test]
fn jsonrpc_error_codes_are_correct() {
    assert_eq!(JsonRpcError::parse_error("").code, -32700);
    assert_eq!(JsonRpcError::invalid_request("").code, -32600);
    assert_eq!(JsonRpcError::method_not_found("x").code, -32601);
    assert_eq!(JsonRpcError::invalid_params("").code, -32602);
    assert_eq!(JsonRpcError::internal("").code, -32603);
}

#[test]
fn jsonrpc_result_produces_valid_envelope() {
    let result = jsonrpc_result(Some(json!(1)), json!({"ok": true}));
    assert_eq!(result["jsonrpc"], "2.0");
    assert_eq!(result["id"], 1);
    assert_eq!(result["result"]["ok"], true);
}

#[test]
fn jsonrpc_error_produces_valid_envelope() {
    let err = jsonrpc_error(Some(json!(2)), JsonRpcError::invalid_params("bad"));
    assert_eq!(err["jsonrpc"], "2.0");
    assert_eq!(err["id"], 2);
    assert_eq!(err["error"]["code"], -32602);
}

// ── McpServerConfig serialization ──────────────────────────────────

#[test]
fn mcp_server_config_defaults_enabled_to_true() {
    let json = json!({"name": "s", "command": "cmd"});
    let config: McpServerConfig =
        serde_json::from_value(json).expect("mcp_server_config_defaults_enabled_to_true");
    assert!(config.enabled);
    assert!(config.args.is_empty());
    assert!(config.env.is_empty());
}

#[test]
fn mcp_startup_status_serializes_with_snake_case() {
    let status = McpStartupStatus::Failed {
        error: "oops".to_string(),
    };
    let json =
        serde_json::to_value(&status).expect("mcp_startup_status_serializes_with_snake_case");
    assert_eq!(json["failed"]["error"], "oops");
}

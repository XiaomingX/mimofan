//! Externalized integration tests for `crates/protocol/src/fleet.rs`.
//!
//! Originally an inline `#[cfg(test)] mod tests` block — relocated here as a
//! separate integration-test crate without any change to test logic.

use mimofan_protocol::fleet::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn fleet_run_round_trip() {
    let run = FleetRun {
        id: FleetRunId::from("run-001"),
        name: "dogfood smoke".to_string(),
        status: FleetRunStatus::Running,
        task_specs: vec![FleetTaskSpec {
            id: "task-1".to_string(),
            name: "lint".to_string(),
            description: None,
            objective: Some("Keep the workspace lint-clean".to_string()),
            instructions: "run cargo clippy".to_string(),
            worker: Some(FleetTaskWorkerProfile {
                agent_profile: None,
                role: Some("release-checker".to_string()),
                loadout: None,
                model_class: None,
                model: None,
                tool_profile: Some("read-only".to_string()),
                tools: vec!["cargo".to_string()],
                capabilities: vec!["rust".to_string()],
            }),
            workspace: Some(FleetWorkspaceRequirements {
                root: Some(PathBuf::from(".")),
                required_files: vec![PathBuf::from("Cargo.toml")],
                writable_paths: vec![],
                environment: Some(FleetEnvironmentRequirements {
                    required: vec!["PATH".to_string()],
                    allowlist: vec!["RUST_LOG".to_string()],
                }),
            }),
            input_files: vec![PathBuf::from("crates/tui/src/main.rs")],
            context: vec!["release gate".to_string()],
            budget: Some(FleetTaskBudget {
                max_tokens: Some(8000),
                max_tool_calls: Some(20),
                max_seconds: Some(300),
            }),
            tags: vec!["release".to_string()],
            expected_artifacts: vec![FleetArtifactKind::Log],
            scorer: Some(FleetScorerSpec::ExitCode),
            retry_policy: Some(FleetRetryPolicy::default()),
            alert_policy: None,
            timeout_seconds: Some(300),
            metadata: BTreeMap::new(),
        }],
        worker_specs: vec![],
        labels: BTreeMap::new(),
        security_policy: None,
        created_at: "2026-06-12T17:00:00Z".to_string(),
        updated_at: None,
        completed_at: None,
    };
    let json = serde_json::to_string(&run).expect("serialize fleet run");
    let back: FleetRun = serde_json::from_str(&json).expect("deserialize fleet run");
    assert_eq!(back.id, run.id);
    assert_eq!(back.status, FleetRunStatus::Running);
    assert_eq!(back.task_specs.len(), 1);
    assert_eq!(
        back.task_specs[0].worker.as_ref().expect("worker profile present").role.as_deref(),
        Some("release-checker")
    );
    assert_eq!(
        back.task_specs[0]
            .workspace
            .as_ref()
            .expect("workspace requirements present")
            .required_files,
        vec![PathBuf::from("Cargo.toml")]
    );
}

#[test]
fn worker_profile_carries_agent_profile_and_loadout_intent() {
    let json = r#"{
        "profile": "adversarial_reviewer",
        "role": "reviewer",
        "loadout": "auto",
        "model_class": "balanced",
        "model": "deepseek-v4-pro",
        "tool_profile": "read-only",
        "tools": ["read_file"],
        "capabilities": ["rust"]
    }"#;

    let profile: FleetTaskWorkerProfile = serde_json::from_str(json).expect("deserialize worker profile");

    assert_eq!(
        profile.agent_profile.as_deref(),
        Some("adversarial_reviewer")
    );
    assert_eq!(profile.role.as_deref(), Some("reviewer"));
    assert_eq!(profile.loadout.as_deref(), Some("auto"));
    assert_eq!(profile.model_class.as_deref(), Some("balanced"));
    assert_eq!(profile.model.as_deref(), Some("deepseek-v4-pro"));
    assert_eq!(profile.tool_profile.as_deref(), Some("read-only"));

    let serialized = serde_json::to_value(&profile).expect("serialize worker profile");
    assert_eq!(serialized["agent_profile"], "adversarial_reviewer");
    assert_eq!(serialized["model"], "deepseek-v4-pro");
    assert!(serialized.get("profile").is_none());
}

#[test]
fn worker_event_lifecycle_round_trip() {
    let events = vec![
        FleetWorkerEvent {
            seq: 1,
            run_id: FleetRunId::from("run-002"),
            worker_id: "worker-a".to_string(),
            task_id: "task-1".to_string(),
            timestamp: "2026-06-12T17:01:00Z".to_string(),
            payload: FleetWorkerEventPayload::Queued,
            extra: BTreeMap::new(),
        },
        FleetWorkerEvent {
            seq: 2,
            run_id: FleetRunId::from("run-002"),
            worker_id: "worker-a".to_string(),
            task_id: "task-1".to_string(),
            timestamp: "2026-06-12T17:01:05Z".to_string(),
            payload: FleetWorkerEventPayload::RunningTool {
                tool: "bash".to_string(),
                call_id: Some("call-1".to_string()),
            },
            extra: BTreeMap::new(),
        },
        FleetWorkerEvent {
            seq: 3,
            run_id: FleetRunId::from("run-002"),
            worker_id: "worker-a".to_string(),
            task_id: "task-1".to_string(),
            timestamp: "2026-06-12T17:02:00Z".to_string(),
            payload: FleetWorkerEventPayload::Completed {
                exit_code: Some(0),
                summary: Some("ok".to_string()),
            },
            extra: BTreeMap::new(),
        },
    ];
    let json = serde_json::to_string(&events).expect("serialize worker events");
    let back: Vec<FleetWorkerEvent> = serde_json::from_str(&json).expect("deserialize worker events");
    assert_eq!(back.len(), 3);
    assert!(matches!(back[0].payload, FleetWorkerEventPayload::Queued));
    assert!(matches!(
        back[2].payload,
        FleetWorkerEventPayload::Completed { .. }
    ));
}

#[test]
fn alert_policy_round_trip() {
    let policy = FleetAlertPolicy {
        events: vec![FleetAlertEventClass::Stale],
        channels: vec![FleetAlertChannel::Slack {
            webhook: FleetAlertEndpoint::inline("https://hooks.slack.com/test"),
        }],
        after_attempts: Some(2),
        after_minutes_stale: Some(10),
    };
    let json = serde_json::to_string(&policy).expect("serialize alert policy");
    assert!(json.contains("\"events\":[\"stale\"]"));
    assert!(json.contains("\"kind\":\"slack\""));
    let back: FleetAlertPolicy = serde_json::from_str(&json).expect("deserialize alert policy");
    assert_eq!(back.events, vec![FleetAlertEventClass::Stale]);
    assert_eq!(back.after_attempts, Some(2));
}

#[test]
fn artifact_other_kind_round_trip() {
    let artifact = FleetArtifactRef {
        kind: FleetArtifactKind::Other("coverage.xml".to_string()),
        path: PathBuf::from("/tmp/coverage.xml"),
        checksum: Some("sha256:abc".to_string()),
        mime_type: Some("application/xml".to_string()),
        size_bytes: Some(1024),
    };
    let json = serde_json::to_string(&artifact).expect("serialize artifact ref");
    let back: FleetArtifactRef = serde_json::from_str(&json).expect("deserialize artifact ref");
    assert_eq!(back.kind, artifact.kind);
    assert_eq!(back.size_bytes, Some(1024));
}

#[test]
fn ssh_host_spec_accepts_minimal_legacy_json() {
    let json = r#"{"kind":"ssh","host":"builder.example.test"}"#;
    let host: FleetHostSpec = serde_json::from_str(json).expect("deserialize ssh host spec");

    match host {
        FleetHostSpec::Ssh {
            host,
            port,
            user,
            identity,
            known_hosts,
            host_key_fingerprint,
            working_directory,
            env_allowlist,
            mimofan_binary,
        } => {
            assert_eq!(host, "builder.example.test");
            assert_eq!(port, None);
            assert_eq!(user, None);
            assert_eq!(identity, None);
            assert_eq!(known_hosts, None);
            assert_eq!(host_key_fingerprint, None);
            assert_eq!(working_directory, None);
            assert!(env_allowlist.is_empty());
            assert_eq!(mimofan_binary, None);
        }
        other => panic!("expected ssh host spec, got {other:?}"),
    }
}

#[test]
fn artifact_kind_uses_flat_string_json() {
    let known = serde_json::to_string(&FleetArtifactKind::TestResult).expect("serialize artifact kind");
    assert_eq!(known, "\"test_result\"");

    let custom =
        serde_json::to_string(&FleetArtifactKind::Other("coverage.xml".to_string())).expect("serialize custom artifact kind");
    assert_eq!(custom, "\"coverage.xml\"");

    let parsed: FleetArtifactKind = serde_json::from_str("\"coverage.xml\"").expect("deserialize artifact kind");
    assert_eq!(parsed, FleetArtifactKind::Other("coverage.xml".to_string()));
}

#[test]
fn retry_policy_missing_fields_use_nonzero_defaults() {
    let policy: FleetRetryPolicy = serde_json::from_value(serde_json::json!({})).expect("deserialize retry policy from empty json");
    assert_eq!(policy, FleetRetryPolicy::default());

    let policy: FleetRetryPolicy =
        serde_json::from_value(serde_json::json!({"max_attempts": 5})).expect("deserialize retry policy with max_attempts");
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(
        policy.initial_backoff_seconds,
        FleetRetryPolicy::default().initial_backoff_seconds
    );
    assert_eq!(
        policy.max_backoff_seconds,
        FleetRetryPolicy::default().max_backoff_seconds
    );
    assert_eq!(
        policy.backoff_multiplier,
        FleetRetryPolicy::default().backoff_multiplier
    );
}

#[test]
fn sparse_worker_events_omit_absent_optional_fields() {
    let heartbeat = FleetWorkerEventPayload::Heartbeat {
        cpu_percent: None,
        memory_mb: None,
    };
    let heartbeat_json = serde_json::to_value(&heartbeat).expect("serialize heartbeat payload");
    assert_eq!(heartbeat_json, serde_json::json!({"state": "heartbeat"}));

    let completed = FleetWorkerEventPayload::Completed {
        exit_code: None,
        summary: None,
    };
    let completed_json = serde_json::to_value(&completed).expect("serialize completed payload");
    assert_eq!(completed_json, serde_json::json!({"state": "completed"}));
}

#[test]
fn receipt_round_trip() {
    let receipt = FleetReceipt {
        run_id: FleetRunId::from("run-003"),
        task_id: "task-1".to_string(),
        worker_id: "worker-b".to_string(),
        completed_at: "2026-06-12T17:03:00Z".to_string(),
        result: FleetTaskResult::Pass,
        failure_kind: None,
        artifacts: vec![],
        score: Some(FleetScore {
            value: 0.95,
            max: Some(1.0),
            notes: None,
        }),
        resolved_route: None,
    };
    let json = serde_json::to_string(&receipt).expect("serialize receipt");
    let back: FleetReceipt = serde_json::from_str(&json).expect("deserialize receipt");
    assert_eq!(back.result, FleetTaskResult::Pass);
    assert_eq!(back.score.as_ref().expect("receipt score present").value, 0.95);
}

#[test]
fn partial_receipt_records_failure_source_when_needed() {
    let receipt = FleetReceipt {
        run_id: FleetRunId::from("run-004"),
        task_id: "task-2".to_string(),
        worker_id: "worker-c".to_string(),
        completed_at: "2026-06-12T17:04:00Z".to_string(),
        result: FleetTaskResult::Partial,
        failure_kind: Some(FleetTaskFailureKind::Verifier),
        artifacts: vec![],
        score: Some(FleetScore {
            value: 0.5,
            max: Some(1.0),
            notes: Some("manual verification required".to_string()),
        }),
        resolved_route: None,
    };

    let json = serde_json::to_string(&receipt).expect("serialize partial receipt");
    assert!(json.contains("\"result\":\"partial\""));
    assert!(json.contains("\"failure_kind\":\"verifier\""));
    let back: FleetReceipt = serde_json::from_str(&json).expect("deserialize partial receipt");
    assert_eq!(back.result, FleetTaskResult::Partial);
    assert_eq!(back.failure_kind, Some(FleetTaskFailureKind::Verifier));
}

#[test]
fn ssh_host_spec_with_key_pinning_round_trip() {
    let spec = FleetHostSpec::Ssh {
        host: "builder.trusted.example.com".to_string(),
        port: Some(22),
        user: Some("mimofan".to_string()),
        identity: Some(PathBuf::from("~/.ssh/mimofan_fleet")),
        known_hosts: Some(PathBuf::from("~/.ssh/known_hosts")),
        host_key_fingerprint: Some("SHA256:aLGqZo1M6c...".to_string()),
        working_directory: Some(PathBuf::from("/srv/mimofan/work")),
        env_allowlist: vec!["MIMOFAN_PROFILE".to_string()],
        mimofan_binary: Some("/usr/local/bin/mimofan".to_string()),
    };
    let json = serde_json::to_string_pretty(&spec).expect("serialize ssh host spec");
    assert!(json.contains("\"known_hosts\""));
    assert!(json.contains("\"host_key_fingerprint\""));
    assert!(json.contains("SHA256:aLGqZo1M6c..."));

    let back: FleetHostSpec = serde_json::from_str(&json).expect("deserialize ssh host spec");
    match back {
        FleetHostSpec::Ssh {
            host,
            known_hosts,
            host_key_fingerprint,
            ..
        } => {
            assert_eq!(host, "builder.trusted.example.com");
            assert_eq!(known_hosts, Some(PathBuf::from("~/.ssh/known_hosts")));
            assert_eq!(
                host_key_fingerprint,
                Some("SHA256:aLGqZo1M6c...".to_string())
            );
        }
        other => panic!("expected ssh host spec, got {other:?}"),
    }
}

#[test]
fn secret_ref_redacted_never_exposes_value() {
    let ref_ = FleetSecretRef::new("MIMOFAN_API_KEY");
    let redacted = ref_.redacted();
    assert!(redacted.contains("MIMOFAN_API_KEY"));
    assert!(!redacted.contains("sk-"));
    assert!(redacted.contains("<secret:"));

    let ref_ = FleetSecretRef::with_source("GH_TOKEN", "env");
    let redacted = ref_.redacted();
    assert!(redacted.contains("env.GH_TOKEN"));
    assert!(!redacted.contains("ghp_"));
}

#[test]
fn alert_endpoint_from_secret_round_trip() {
    let endpoint = FleetAlertEndpoint::from_secret(FleetSecretRef::new("SLACK_WEBHOOK"));
    let json = serde_json::to_string(&endpoint).expect("serialize alert endpoint");
    assert!(json.contains("SLACK_WEBHOOK"));
    assert!(!json.contains("hooks.slack.com"));

    let back: FleetAlertEndpoint = serde_json::from_str(&json).expect("deserialize alert endpoint");
    assert_eq!(back.url_ref.as_ref().expect("alert endpoint url_ref present").key, "SLACK_WEBHOOK");
    assert_eq!(back.url, None);
}

#[test]
fn secret_ref_accepts_legacy_string_wire_shape() {
    let ref_: FleetSecretRef = serde_json::from_str(r#""MIMOFAN_FLEET_TOKEN""#).expect("deserialize secret ref from legacy string");
    assert_eq!(ref_, FleetSecretRef::new("MIMOFAN_FLEET_TOKEN"));

    let ref_: FleetSecretRef =
        serde_json::from_str(r#"{"key":"GH_TOKEN","source":"env"}"#).expect("deserialize secret ref with source");
    assert_eq!(ref_, FleetSecretRef::with_source("GH_TOKEN", "env"));
}

#[test]
fn trust_level_accepts_hyphenated_remote_verified() {
    let trust: FleetTrustLevel = serde_json::from_str(r#""remote-verified""#).expect("deserialize trust level");
    assert_eq!(trust, FleetTrustLevel::RemoteVerified);

    let canonical = serde_json::to_string(&trust).expect("serialize trust level");
    assert_eq!(canonical, r#""remote_verified""#);
}

#[test]
fn alert_channel_accepts_legacy_webhook_fields() {
    let channel: FleetAlertChannel = serde_json::from_str(
        r#"{
            "kind": "slack",
            "webhook_url": "https://hooks.slack.com/test",
            "secret": "SLACK_SIGNING_SECRET"
        }"#,
    )
    .expect("deserialize slack alert channel");

    match channel {
        FleetAlertChannel::Slack { webhook } => {
            assert_eq!(webhook.url.as_deref(), Some("https://hooks.slack.com/test"));
            assert_eq!(
                webhook.secret_ref,
                Some(FleetSecretRef::new("SLACK_SIGNING_SECRET"))
            );
        }
        other => panic!("expected slack channel, got {other:?}"),
    }
}

#[test]
fn security_policy_defaults_are_conservative() {
    let policy = FleetSecurityPolicy::default();
    assert_eq!(policy.default_trust_level, FleetTrustLevel::Sandbox);
    assert!(policy.allowed_secrets.is_empty());
    assert!(policy.capability_grants.is_empty());
    assert_eq!(policy.max_trust_level, FleetTrustLevel::Operator);
    assert!(!policy.require_identity_verification);
}

#[test]
fn trust_level_ordinal_reflects_privilege() {
    assert!(FleetTrustLevel::Operator > FleetTrustLevel::RemoteVerified);
    assert!(FleetTrustLevel::RemoteVerified > FleetTrustLevel::Local);
    assert!(FleetTrustLevel::Local > FleetTrustLevel::Sandbox);

    assert!(FleetTrustLevel::Operator.may_access_secrets());
    assert!(!FleetTrustLevel::Sandbox.may_access_secrets());
    assert!(!FleetTrustLevel::Sandbox.may_write_workspace());
    assert!(FleetTrustLevel::Operator.may_write_workspace());
}

fn sample_receipt_with_route() -> FleetReceipt {
    FleetReceipt {
        run_id: FleetRunId::from("run-route"),
        task_id: "task-route".to_string(),
        worker_id: "worker-route".to_string(),
        completed_at: "2026-06-23T00:00:00Z".to_string(),
        result: FleetTaskResult::Pass,
        failure_kind: None,
        artifacts: vec![],
        score: None,
        resolved_route: Some(FleetResolvedRoute {
            provider_id: "deepseek".to_string(),
            provider_kind: "deepseek".to_string(),
            canonical_model: Some("deepseek-v4-pro".to_string()),
            wire_model_id: "deepseek-v4-pro".to_string(),
            protocol: "chat_completions".to_string(),
            role: Some("builder".to_string()),
            loadout: Some("auto".to_string()),
            source: "resolver".to_string(),
        }),
    }
}

#[test]
fn fleet_resolved_route_round_trips() {
    let receipt = sample_receipt_with_route();
    let json = serde_json::to_string(&receipt).expect("serialize resolved-route receipt");
    let back: FleetReceipt = serde_json::from_str(&json).expect("deserialize resolved-route receipt");
    assert_eq!(back.resolved_route, receipt.resolved_route);
    let route = back.resolved_route.expect("resolved route present");
    assert_eq!(route.provider_id, "deepseek");
    assert_eq!(route.wire_model_id, "deepseek-v4-pro");
    assert_eq!(route.protocol, "chat_completions");
    assert_eq!(route.role.as_deref(), Some("builder"));
    assert_eq!(route.source, "resolver");
}

#[test]
fn fleet_receipt_without_resolved_route_still_deserializes() {
    // An old ledger receipt JSON written before #3154 has no
    // `resolved_route` key; `#[serde(default)]` must keep it readable.
    let legacy = r#"{
        "run_id": "run-legacy",
        "task_id": "task-legacy",
        "worker_id": "worker-legacy",
        "completed_at": "2026-06-01T00:00:00Z",
        "result": "pass",
        "artifacts": [],
        "score": null
    }"#;
    let receipt: FleetReceipt = serde_json::from_str(legacy).expect("deserialize legacy receipt");
    assert_eq!(receipt.task_id, "task-legacy");
    assert!(receipt.resolved_route.is_none());
}

#[test]
fn fleet_resolved_route_serialization_carries_no_secrets() {
    let receipt = sample_receipt_with_route();
    // Scan the serialized resolved-route object: this is the field whose
    // no-secrets invariant we are asserting. Scoping to the route value
    // avoids false positives from unrelated envelope ids (e.g. a task id
    // such as "task-foo" innocently contains the substring "sk-").
    let route_json = serde_json::to_string(receipt.resolved_route.as_ref().expect("resolved route present for secret scan")).expect("serialize resolved route for secret scan");
    assert_no_secret_markers(&route_json);
    // The envelope as a whole must also stay credential-free.
    let receipt_json = serde_json::to_string(&receipt).expect("serialize receipt for secret scan");
    for needle in SECRET_KEY_MARKERS {
        assert!(
            !receipt_json.to_ascii_lowercase().contains(needle),
            "receipt JSON must not contain secret-key marker {needle:?}: {receipt_json}"
        );
    }
}

/// Substrings that indicate a leaked credential field/value. These are
/// deliberately specific so legitimate ids/model names do not trip them.
const SECRET_KEY_MARKERS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "bearer ",
    "auth_token",
    "auth-token",
    "password",
    "credential",
    "sk-ant-",
    "sk-proj-",
    "sk-or-",
    "secret",
];

fn assert_no_secret_markers(json: &str) {
    let haystack = json.to_ascii_lowercase();
    for needle in SECRET_KEY_MARKERS {
        assert!(
            !haystack.contains(needle),
            "resolved-route JSON must not contain secret marker {needle:?}: {json}"
        );
    }
}

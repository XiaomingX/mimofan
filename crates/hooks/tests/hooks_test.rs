use anyhow::Result;
use async_trait::async_trait;
use mimofan_hooks::*;
use mimofan_protocol::EventFrame;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn hook_event_serializes_with_snake_case_type_and_payload() {
    let event = HookEvent::ToolLifecycle {
        response_id: "resp-1".to_string(),
        tool_name: "shell".to_string(),
        phase: "end".to_string(),
        payload: json!({ "exit_code": 0 }),
    };

    let encoded = event.to_json();

    assert_eq!(encoded["type"], "tool_lifecycle");
    assert_eq!(encoded["response_id"], "resp-1");
    assert_eq!(encoded["tool_name"], "shell");
    assert_eq!(encoded["phase"], "end");
    assert_eq!(encoded["payload"]["exit_code"], 0);
}

#[test]
fn generic_event_frame_serialization_is_unchanged_by_boxing() {
    let event = HookEvent::GenericEventFrame {
        frame: Box::new(EventFrame::ResponseStart {
            response_id: "resp-1".to_string(),
        }),
    };

    let encoded = event.to_json();

    assert_eq!(encoded["type"], "generic_event_frame");
    assert_eq!(encoded["frame"]["event"], "response_start");
    assert_eq!(encoded["frame"]["response_id"], "resp-1");
}

#[tokio::test]
async fn jsonl_sink_creates_parent_dir_and_appends_events() {
    let root = unique_temp_dir("jsonl_sink");
    let path = root.join("nested").join("hooks.jsonl");
    let sink = JsonlHookSink::new(path.clone());

    sink.emit(&HookEvent::ResponseStart {
        response_id: "resp-1".to_string(),
    })
    .await
    .expect("emit response_start event to jsonl sink");
    sink.emit(&HookEvent::ResponseEnd {
        response_id: "resp-1".to_string(),
    })
    .await
    .expect("emit response_end event to jsonl sink");

    let raw = std::fs::read_to_string(&path).expect("read jsonl hook log file");
    let lines = raw.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);

    let first: Value = serde_json::from_str(lines[0]).expect("parse first jsonl line");
    let second: Value = serde_json::from_str(lines[1]).expect("parse second jsonl line");
    assert!(first["at"].as_str().is_some());
    assert_eq!(first["event"]["type"], "response_start");
    assert_eq!(first["event"]["response_id"], "resp-1");
    assert_eq!(second["event"]["type"], "response_end");
    assert_eq!(second["event"]["response_id"], "resp-1");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn dispatcher_continues_after_sink_error() {
    let mut dispatcher = HookDispatcher::default();
    let first = Arc::new(RecordingSink::default());
    let second = Arc::new(RecordingSink::default());

    dispatcher.add_sink(first.clone());
    dispatcher.add_sink(Arc::new(FailingSink));
    dispatcher.add_sink(second.clone());

    dispatcher
        .emit(HookEvent::ApprovalLifecycle {
            approval_id: "approval-1".to_string(),
            phase: "requested".to_string(),
            reason: Some("needs review".to_string()),
        })
        .await;

    assert_eq!(
        first.events(),
        vec![json!({
            "type": "approval_lifecycle",
            "approval_id": "approval-1",
            "phase": "requested",
            "reason": "needs review",
        })]
    );
    assert_eq!(second.events(), first.events());
}

#[cfg(unix)]
#[tokio::test]
async fn unix_socket_sink_skips_when_listener_absent() {
    let (root, socket_path) = unique_short_socket_path("missing");
    let sink = UnixSocketHookSink::new(socket_path);
    let result = sink
        .emit(&HookEvent::ResponseStart {
            response_id: "resp-1".to_string(),
        })
        .await;
    assert!(result.is_ok());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn unix_socket_sink_sends_event_to_listener() {
    use tokio::io::AsyncBufReadExt;
    use tokio::net::UnixListener;

    let (root, socket_path) = unique_short_socket_path("send");
    std::fs::create_dir_all(&root).expect("mkdir");
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path).expect("bind");
    let sink = UnixSocketHookSink::new(socket_path.clone());

    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut reader = tokio::io::BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read_line");
        line
    });

    sink.emit(&HookEvent::ResponseStart {
        response_id: "resp-42".to_string(),
    })
    .await
    .expect("emit");

    let received = handle.await.expect("join");
    let parsed: Value = serde_json::from_str(&received).expect("parse");
    assert_eq!(parsed["event"]["type"], "response_start");
    assert_eq!(parsed["event"]["response_id"], "resp-42");
    assert!(parsed["at"].as_str().is_some());

    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_dir_all(root);
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<Value>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<Value> {
        self.events
            .lock()
            .expect("lock recording sink events")
            .clone()
    }
}

#[async_trait]
impl HookSink for RecordingSink {
    async fn emit(&self, event: &HookEvent) -> Result<()> {
        self.events
            .lock()
            .expect("lock recording sink events")
            .push(event.to_json());
        Ok(())
    }
}

struct FailingSink;

#[async_trait]
impl HookSink for FailingSink {
    async fn emit(&self, _event: &HookEvent) -> Result<()> {
        anyhow::bail!("sink failed")
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is valid since unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deepseek-hooks-{label}-{}-{nanos}",
        std::process::id()
    ))
}

#[cfg(unix)]
fn unique_short_socket_path(label: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is valid since unix epoch")
        .as_nanos();
    let root = PathBuf::from("/tmp").join(format!("cw-hk-{}-{nanos}", std::process::id()));
    let path = root.join(format!("{label}.sock"));
    (root, path)
}

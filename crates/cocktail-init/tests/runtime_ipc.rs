//! Real child-process framing and runtime event routing (run in CI).
use cocktail_shared::model::{InstanceEvent, InstanceStatus};
use cocktail_shared::runtime::{INSTANCE_EVENT, RuntimeEvent};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{ChildStdin, ChildStdout, Command},
};

async fn request(stdin: &mut ChildStdin, id: u64, method: &str, params: Value) {
    let body = serde_json::to_vec(
        &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
    .unwrap();
    stdin
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .unwrap();
    stdin.write_all(&body).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn response_and_event(
    stdout: &mut ChildStdout,
    id: u64,
    token: &str,
    predicate: impl Fn(&InstanceEvent) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut result = None;
        let mut observed = false;
        loop {
            let frame = cocktail_init::server::read_frame(stdout).await.unwrap();
            let value: Value = serde_json::from_slice(&frame).unwrap();
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                assert!(value["error"].is_null(), "RPC failed: {value}");
                result = Some(value["result"].clone());
            } else if value["method"] == INSTANCE_EVENT {
                let event: RuntimeEvent = serde_json::from_value(value["params"].clone()).unwrap();
                assert_eq!(event.token, token);
                observed |= predicate(&event.event);
            }
            if observed {
                if let Some(result) = result.take() {
                    break result;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for both response and runtime event")
}

#[tokio::test]
async fn demo_lifecycle_across_real_init_pipe() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cocktail-init"));
    cmd.current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    cocktail_shared::wincompat::hide_console(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let token = uuid::Uuid::new_v4().to_string();
    request(
        &mut stdin,
        1,
        "process.launch",
        json!({
            "token": token, "instance_id": "ipc-demo", "workdir": dir.path().join("instance"),
            "port": 25565, "kind": "process", "command": null, "args": [], "memory_mib": 512
        }),
    )
    .await;
    let info = response_and_event(&mut stdout, 1, &token, |event| {
        matches!(
            event,
            InstanceEvent::StatusChanged {
                status: InstanceStatus::Running,
                ..
            }
        )
    })
    .await;
    assert_eq!(info["token"], token);
    request(
        &mut stdin,
        2,
        "process.command",
        json!({"token": token, "command":"say batch2-ipc"}),
    )
    .await;
    response_and_event(&mut stdout, 2, &token, |event| {
        matches!(event,
        InstanceEvent::Log { line, .. } if line.line.contains("batch2-ipc"))
    })
    .await;
    request(
        &mut stdin,
        3,
        "process.stop",
        json!({"token": token, "mode":"Graceful"}),
    )
    .await;
    response_and_event(&mut stdout, 3, &token, |event| {
        matches!(
            event,
            InstanceEvent::StatusChanged {
                status: InstanceStatus::Stopped,
                ..
            }
        )
    })
    .await;
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
}

use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tempfile::TempDir;

fn read_to_string_best_effort(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn wait_for_daemon_ready(dir: &TempDir) {
    let log_path = dir.path().join("fugue.log");
    let sock_path = dir.path().join("fugue.sock");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let log = read_to_string_best_effort(&log_path);
            panic!("timed out waiting for daemon ready; log was: {log}");
        }

        if sock_path.exists() {
            let log = read_to_string_best_effort(&log_path);
            if log.contains("daemon ready") {
                return;
            }
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn spawn_daemon(dir: &TempDir) -> std::process::Child {
    let child = Command::new(assert_cmd::cargo::cargo_bin!("fugue"))
        .env("FUGUE_DIR", dir.path())
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(dir);
    child
}

#[test]
fn ping_and_shutdown_work_over_ipc() {
    let dir = TempDir::new().unwrap();
    let mut child = spawn_daemon(&dir);

    let mut ping = cargo_bin_cmd!("fugue");
    ping.env("FUGUE_DIR", dir.path());
    ping.arg("ping");
    ping.assert().success().stdout("ok\n");

    let mut shutdown = cargo_bin_cmd!("fugue");
    shutdown.env("FUGUE_DIR", dir.path());
    shutdown.args(["server", "shutdown"]);
    shutdown.assert().success().stdout("ok\n");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "status: {status:?}");
            break;
        }
        if Instant::now() > deadline {
            let pid = Pid::from_raw(child.id() as i32);
            let _ = kill(pid, Signal::SIGKILL);
            panic!("timed out waiting for daemon to exit after shutdown");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test]
async fn attach_receives_heartbeat_event() {
    let dir = TempDir::new().unwrap();
    let mut child = spawn_daemon(&dir);

    let socket_path = dir.path().join("fugue.sock");
    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (read_half, write_half) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut writer = tokio::io::BufWriter::new(write_half);

    let attach = fugue_protocol::Request {
        r#type: fugue_protocol::MSG_ATTACH.to_owned(),
        id: "attach-1".to_owned(),
        payload: serde_json::json!({ "projects": [] }),
    };
    fugue::ipc::jsonl::write_jsonl(&mut writer, &attach)
        .await
        .unwrap();

    let resp: fugue_protocol::Response = fugue::ipc::jsonl::read_jsonl(&mut reader)
        .await
        .unwrap()
        .unwrap();
    assert!(resp.success);
    assert_eq!(resp.r#type, fugue_protocol::MSG_ATTACH);

    let event: fugue_protocol::Event = tokio::time::timeout(Duration::from_secs(3), async {
        fugue::ipc::jsonl::read_jsonl(&mut reader)
            .await
            .unwrap()
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(event.r#type, fugue_protocol::EVT_HEARTBEAT);

    let detach = fugue_protocol::Request {
        r#type: fugue_protocol::MSG_DETACH.to_owned(),
        id: "detach-1".to_owned(),
        payload: serde_json::Value::Null,
    };
    fugue::ipc::jsonl::write_jsonl(&mut writer, &detach)
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for detach response");
        }

        let msg: serde_json::Value = tokio::time::timeout(Duration::from_secs(1), async {
            fugue::ipc::jsonl::read_jsonl(&mut reader)
                .await
                .unwrap()
                .unwrap()
        })
        .await
        .unwrap();

        if msg.get("type") == Some(&serde_json::Value::String("detach".to_owned())) {
            let resp: fugue_protocol::Response = serde_json::from_value(msg).unwrap();
            assert!(resp.success);
            break;
        }
    }

    drop(writer);
    drop(reader);

    let mut shutdown = cargo_bin_cmd!("fugue");
    shutdown.env("FUGUE_DIR", dir.path());
    shutdown.args(["server", "shutdown"]);
    shutdown.assert().success();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "status: {status:?}");
            break;
        }
        if Instant::now() > deadline {
            let pid = Pid::from_raw(child.id() as i32);
            let _ = kill(pid, Signal::SIGKILL);
            panic!("timed out waiting for daemon to exit after shutdown");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

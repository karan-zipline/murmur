use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use hmac::{Hmac, Mac as _};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use sha2::Sha256;
use tempfile::TempDir;
use tokio::io::{BufReader, BufWriter};
use tokio::net::UnixStream;

use fugue::ipc::jsonl::{read_jsonl, write_jsonl};

fn read_to_string_best_effort(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn setup_fake_binaries() -> TempDir {
    let dir = TempDir::new().unwrap();
    let bin_dir = dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_executable(
        &bin_dir.join("claude"),
        r#"#!/usr/bin/env bash
set -euo pipefail
while IFS= read -r line; do
  if [[ -z "${line// }" ]]; then
    continue
  fi
  echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"(fake claude) ok"}]}}'
done
"#,
    );

    dir
}

fn wait_for_daemon_ready(dir: &TempDir) {
    let log_path = dir.path().join("fugue.log");
    let sock_path = dir.path().join("fugue.sock");

    let deadline = Instant::now() + Duration::from_secs(10);
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

fn wait_for_webhook_addr(dir: &TempDir) -> String {
    let log_path = dir.path().join("fugue.log");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            let log = read_to_string_best_effort(&log_path);
            panic!("timed out waiting for webhook server; log was: {log}");
        }

        let log = read_to_string_best_effort(&log_path);
        if let Some(addr) = extract_webhook_addr(&log) {
            return addr;
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn extract_webhook_addr(log: &str) -> Option<String> {
    for line in log.lines().rev() {
        if !line.contains("webhook server started") {
            continue;
        }
        for part in line.split_whitespace() {
            if let Some(rest) = part.strip_prefix("addr=") {
                return Some(rest.trim_matches(',').to_owned());
            }
        }
    }
    None
}

fn spawn_daemon(dir: &TempDir, bin_dir: &Path) -> std::process::Child {
    let path = {
        let mut parts = Vec::new();
        parts.push(bin_dir.to_string_lossy().to_string());
        if let Some(existing) = env::var_os("PATH") {
            parts.push(existing.to_string_lossy().to_string());
        }
        parts.join(":")
    };

    let child = Command::new(assert_cmd::cargo::cargo_bin!("fugue"))
        .env("FUGUE_DIR", dir.path())
        .env("PATH", path)
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(dir);
    child
}

fn shutdown_daemon(dir: &TempDir, mut child: std::process::Child) {
    let mut shutdown = cargo_bin_cmd!("fugue");
    shutdown.env("FUGUE_DIR", dir.path());
    shutdown.args(["server", "shutdown"]);
    shutdown.assert().success();

    let deadline = Instant::now() + Duration::from_secs(10);
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

fn write_webhook_config(dir: &TempDir, bind_addr: &str, secret: &str) {
    let cfg_dir = dir.path().join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::write(
        cfg_dir.join("config.toml"),
        format!(
            r#"
[webhook]
enabled = true
bind-addr = "{bind_addr}"
path-prefix = "/webhooks"
secret = "{secret}"
"#
        ),
    )
    .unwrap();
}

fn hmac_sha256_hex(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    hex::encode(tag)
}

async fn wait_for_tick_event<F, Fut>(
    dir: &TempDir,
    expected_project: &str,
    expected_source: &str,
    trigger: F,
) -> anyhow::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    let stream = UnixStream::connect(dir.path().join("fugue.sock")).await?;
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);

    let attach = fugue_protocol::Request {
        r#type: fugue_protocol::MSG_ATTACH.to_owned(),
        id: "t-attach".to_owned(),
        payload: serde_json::to_value(fugue_protocol::AttachRequest::default())?,
    };
    write_jsonl(&mut writer, &attach).await?;

    let _resp: serde_json::Value = read_jsonl(&mut reader).await?.unwrap_or_default();

    trigger().await?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            return Err(anyhow::anyhow!(
                "timed out waiting for tick requested event"
            ));
        }

        let maybe_value = match tokio::time::timeout(
            Duration::from_millis(500),
            read_jsonl::<_, serde_json::Value>(&mut reader),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => None,
        };

        let Some(value) = maybe_value else {
            continue;
        };

        if value.get("success").is_some() {
            continue;
        }

        let evt: fugue_protocol::Event = match serde_json::from_value(value) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if evt.r#type != fugue_protocol::EVT_ORCHESTRATION_TICK_REQUESTED {
            continue;
        }

        let payload: fugue_protocol::OrchestrationTickRequestedEvent =
            serde_json::from_value(evt.payload)?;
        assert_eq!(payload.project, expected_project);
        assert_eq!(payload.source, expected_source);
        return Ok(());
    }
}

#[tokio::test]
async fn webhook_health_returns_ok() {
    let fugue_dir = TempDir::new().unwrap();
    write_webhook_config(&fugue_dir, "127.0.0.1:0", "secret");
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&fugue_dir, &bins.path().join("bin"));

    let addr = wait_for_webhook_addr(&fugue_dir);
    let url = format!("http://{addr}/health");

    let resp = reqwest::get(url).await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "ok");

    shutdown_daemon(&fugue_dir, daemon);
}

#[tokio::test]
async fn github_webhook_emits_tick_requested_event() {
    let fugue_dir = TempDir::new().unwrap();
    let secret = "sekret";
    write_webhook_config(&fugue_dir, "127.0.0.1:0", secret);
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&fugue_dir, &bins.path().join("bin"));

    let addr = wait_for_webhook_addr(&fugue_dir);
    let url = format!("http://{addr}/webhooks/github?project=demo");

    let body = br#"{"action":"opened"}"#.to_vec();
    let sig = format!("sha256={}", hmac_sha256_hex(secret, &body));

    let client = reqwest::Client::new();
    wait_for_tick_event(&fugue_dir, "demo", "github", || async {
        let resp = client
            .post(url.as_str())
            .header("X-GitHub-Event", "issues")
            .header("X-GitHub-Delivery", "delivery-1")
            .header("X-Hub-Signature-256", sig.as_str())
            .body(body.clone())
            .send()
            .await?;
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        Ok(())
    })
    .await
    .unwrap();

    shutdown_daemon(&fugue_dir, daemon);
}

#[tokio::test]
async fn linear_webhook_emits_tick_requested_event() {
    let fugue_dir = TempDir::new().unwrap();
    let secret = "sekret";
    write_webhook_config(&fugue_dir, "127.0.0.1:0", secret);
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&fugue_dir, &bins.path().join("bin"));

    let addr = wait_for_webhook_addr(&fugue_dir);
    let url = format!("http://{addr}/webhooks/linear?project=demo");

    let body = br#"{"action":"create","type":"Issue"}"#.to_vec();
    let sig = hmac_sha256_hex(secret, &body);

    let client = reqwest::Client::new();
    wait_for_tick_event(&fugue_dir, "demo", "linear", || async {
        let resp = client
            .post(url.as_str())
            .header("Linear-Signature", sig.as_str())
            .body(body.clone())
            .send()
            .await?;
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        Ok(())
    })
    .await
    .unwrap();

    shutdown_daemon(&fugue_dir, daemon);
}

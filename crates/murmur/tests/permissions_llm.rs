use std::env;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tempfile::TempDir;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn read_to_string_best_effort(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed", args);
}

fn init_local_remote(base: &Path) -> PathBuf {
    let origin = base.join("origin.git");
    run_git(base, &["init", "--bare", origin.to_str().unwrap()]);

    let seed = base.join("seed");
    run_git(
        base,
        &["clone", origin.to_str().unwrap(), seed.to_str().unwrap()],
    );
    run_git(&seed, &["checkout", "-b", "main"]);
    fs::write(seed.join("README.md"), "hello\n").unwrap();
    run_git(&seed, &["add", "."]);
    run_git(
        &seed,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "init",
        ],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    origin
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

fn write_llm_config(dir: &TempDir) {
    let cfg_dir = dir.path().join("config");
    fs::create_dir_all(&cfg_dir).unwrap();
    let cfg_path = cfg_dir.join("config.toml");
    fs::write(
        cfg_path,
        r#"
[llm_auth]
provider = "openai"
model = "gpt-test"

[providers.openai]
api-key = "test-key"
"#,
    )
    .unwrap();
}

fn wait_for_daemon_ready(dir: &TempDir) {
    let log_path = dir.path().join("murmur.log");
    let sock_path = dir.path().join("murmur.sock");

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

fn spawn_daemon(dir: &TempDir, bin_dir: &Path, openai_url: &str) -> std::process::Child {
    let path = {
        let mut parts = Vec::new();
        parts.push(bin_dir.to_string_lossy().to_string());
        if let Some(existing) = env::var_os("PATH") {
            parts.push(existing.to_string_lossy().to_string());
        }
        parts.join(":")
    };

    let child = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", dir.path())
        .env("PATH", path)
        .env("OPENAI_API_URL", openai_url)
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(dir);
    child
}

fn shutdown_daemon(dir: &TempDir, mut child: std::process::Child) {
    let mut shutdown = cargo_bin_cmd!("mm");
    shutdown.env("MURMUR_DIR", dir.path());
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

fn add_project(murmur_dir: &TempDir, origin: &Path) {
    let mut add = cargo_bin_cmd!("mm");
    add.env("MURMUR_DIR", murmur_dir.path());
    add.args([
        "project",
        "add",
        "demo",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add.assert().success().stdout("ok\n");
}

fn create_agent(murmur_dir: &TempDir) -> String {
    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["agent", "create", "demo", "ISSUE-1"]);
    let out = create.assert().success().get_output().stdout.clone();
    String::from_utf8_lossy(&out).trim().to_owned()
}

fn worktree_dir(murmur_dir: &TempDir, agent_id: &str) -> PathBuf {
    murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join(format!("wt-{agent_id}"))
}

fn enable_llm_checker(murmur_dir: &TempDir) {
    let mut set = cargo_bin_cmd!("mm");
    set.env("MURMUR_DIR", murmur_dir.path());
    set.args([
        "project",
        "config",
        "set",
        "demo",
        "permissions-checker",
        "llm",
    ]);
    set.assert().success().stdout("ok\n");
}

#[tokio::test]
async fn hook_pre_tool_use_allows_when_llm_says_safe() {
    let openai = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .and(body_string_contains("authorization_decision"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "arguments": "{\"decision\":\"safe\",\"rationale\":\"ok\"}"
                        }
                    }]
                }
            }]
        })))
        .mount(&openai)
        .await;

    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    write_llm_config(&murmur_dir);

    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"), &openai.uri());

    add_project(&murmur_dir, &origin);
    enable_llm_checker(&murmur_dir);

    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");
    let cwd = worktree_dir(&murmur_dir, &agent_id);

    let input = serde_json::json!({
        "session_id": "s-1",
        "transcript_path": "",
        "cwd": cwd.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "echo hello" },
        "tool_use_id": "tu-1"
    });

    let output = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .env("MURMUR_AGENT_ID", &agent_id)
        .env("MURMUR_PROJECT", "demo")
        .args(["hook", "PreToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut child = output;
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(format!("{input}\n").as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let decision = json["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(decision, "allow");

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["permission", "list"]);
    let out = list.assert().success().get_output().stdout.clone();
    assert!(String::from_utf8_lossy(&out).trim().is_empty());

    shutdown_daemon(&murmur_dir, daemon);
}

#[tokio::test]
async fn hook_pre_tool_use_denies_when_llm_unsure() {
    let openai = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": {
                            "arguments": "{\"decision\":\"unsure\",\"rationale\":\"need review\"}"
                        }
                    }]
                }
            }]
        })))
        .mount(&openai)
        .await;

    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    write_llm_config(&murmur_dir);

    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"), &openai.uri());

    add_project(&murmur_dir, &origin);
    enable_llm_checker(&murmur_dir);

    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");
    let cwd = worktree_dir(&murmur_dir, &agent_id);

    let input = serde_json::json!({
        "session_id": "s-1",
        "transcript_path": "",
        "cwd": cwd.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": "echo hello" },
        "tool_use_id": "tu-1"
    });

    let mut hook = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .env("MURMUR_AGENT_ID", &agent_id)
        .env("MURMUR_PROJECT", "demo")
        .args(["hook", "PreToolUse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    {
        let mut stdin = hook.stdin.take().unwrap();
        stdin.write_all(format!("{input}\n").as_bytes()).unwrap();
    }

    let out = hook.wait_with_output().unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let decision = json["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(decision, "deny");

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["permission", "list"]);
    let out = list.assert().success().get_output().stdout.clone();
    assert!(String::from_utf8_lossy(&out).trim().is_empty());

    shutdown_daemon(&murmur_dir, daemon);
}

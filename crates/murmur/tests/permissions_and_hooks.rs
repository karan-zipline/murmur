use std::env;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use murmur::ipc::jsonl::{read_jsonl, write_jsonl};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tempfile::TempDir;

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

settings=""
while [[ $# -gt 0 ]]; do
  if [[ "${1:-}" == "--settings" ]]; then
    settings="${2:-}"
    shift 2
    continue
  fi
  shift
done

if [[ -n "${FAKE_CLAUDE_SETTINGS_OUT:-}" ]]; then
  printf "%s" "${settings}" > "${FAKE_CLAUDE_SETTINGS_OUT}"
fi

if [[ -n "${FAKE_CLAUDE_ENV_OUT:-}" ]]; then
  printf "%s" "${MURMUR_DIR:-}" > "${FAKE_CLAUDE_ENV_OUT}"
fi

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

fn spawn_daemon(dir: &TempDir, bin_dir: &Path) -> std::process::Child {
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

#[test]
fn claude_settings_include_shell_safe_hook_commands() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let settings_out = murmur_dir.path().join("claude-settings.json");
    let bins = setup_fake_binaries();

    let path = {
        let mut parts = Vec::new();
        parts.push(bins.path().join("bin").to_string_lossy().to_string());
        if let Some(existing) = env::var_os("PATH") {
            parts.push(existing.to_string_lossy().to_string());
        }
        parts.join(":")
    };

    let daemon = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .env("PATH", path)
        .env("FAKE_CLAUDE_SETTINGS_OUT", &settings_out)
        .env("FUGUE_HOOK_EXE", "/tmp/mm hook exe (deleted)")
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(&murmur_dir);
    add_project(&murmur_dir, &origin);
    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if settings_out.exists() {
            break;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for fake claude to write settings");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let settings_raw = fs::read_to_string(&settings_out).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&settings_raw).unwrap();

    let hooks = &settings["hooks"];
    let pre = &hooks["PreToolUse"][0]["hooks"][0];
    let perm = &hooks["PermissionRequest"][0]["hooks"][0];
    let stop = &hooks["Stop"][0]["hooks"][0];

    assert_eq!(pre["timeout"].as_i64().unwrap(), 300);
    assert_eq!(perm["timeout"].as_i64().unwrap(), 300);
    assert_eq!(stop["timeout"].as_i64().unwrap(), 10);

    let pre_cmd = pre["command"].as_str().unwrap();
    let perm_cmd = perm["command"].as_str().unwrap();
    let stop_cmd = stop["command"].as_str().unwrap();

    assert!(!pre_cmd.contains(" (deleted)"));
    assert!(!perm_cmd.contains(" (deleted)"));
    assert!(!stop_cmd.contains(" (deleted)"));

    assert_eq!(pre_cmd, "'/tmp/mm hook exe' 'hook' 'PreToolUse'");
    assert_eq!(perm_cmd, "'/tmp/mm hook exe' 'hook' 'PermissionRequest'");
    assert_eq!(stop_cmd, "'/tmp/mm hook exe' 'hook' 'Stop'");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn spawned_agents_receive_fugue_dir_env() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let env_out = murmur_dir.path().join("claude-env.txt");
    let bins = setup_fake_binaries();

    let path = {
        let mut parts = Vec::new();
        parts.push(bins.path().join("bin").to_string_lossy().to_string());
        if let Some(existing) = env::var_os("PATH") {
            parts.push(existing.to_string_lossy().to_string());
        }
        parts.join(":")
    };

    let daemon = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .env("PATH", path)
        .env("FAKE_CLAUDE_ENV_OUT", &env_out)
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(&murmur_dir);
    add_project(&murmur_dir, &origin);

    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if env_out.exists() {
            break;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for fake claude to write env");
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let got = read_to_string_best_effort(&env_out);
    assert_eq!(got.trim(), murmur_dir.path().to_string_lossy());

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn hook_pre_tool_use_blocks_until_permission_respond() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

    add_project(&murmur_dir, &origin);
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

    let deadline = Instant::now() + Duration::from_secs(5);
    let request_id = loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for permission request to appear");
        }

        let mut list = cargo_bin_cmd!("mm");
        list.env("MURMUR_DIR", murmur_dir.path());
        list.args(["permission", "list"]);
        let out = list.assert().success().get_output().stdout.clone();
        let out = String::from_utf8_lossy(&out).to_string();
        if let Some(line) = out.lines().next().filter(|l| !l.trim().is_empty()) {
            break line.split('\t').next().unwrap().to_owned();
        }

        std::thread::sleep(Duration::from_millis(50));
    };

    let mut respond = cargo_bin_cmd!("mm");
    respond.env("MURMUR_DIR", murmur_dir.path());
    respond.args(["permission", "respond", &request_id, "allow"]);
    respond.assert().success().stdout("ok\n");

    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let decision = json["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(decision, "allow");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn hook_ask_user_question_updates_input_with_answers() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

    add_project(&murmur_dir, &origin);
    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");
    let cwd = worktree_dir(&murmur_dir, &agent_id);

    let input = serde_json::json!({
        "session_id": "s-1",
        "transcript_path": "",
        "cwd": cwd.to_string_lossy(),
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "tool_input": {
            "questions": [
                {
                    "question": "Pick one",
                    "header": "choice",
                    "multiSelect": false,
                    "options": [
                        { "label": "A", "description": "Option A" },
                        { "label": "B", "description": "Option B" }
                    ]
                }
            ]
        },
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

    let deadline = Instant::now() + Duration::from_secs(5);
    let request_id = loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for question request to appear");
        }

        let mut list = cargo_bin_cmd!("mm");
        list.env("MURMUR_DIR", murmur_dir.path());
        list.args(["question", "list"]);
        let out = list.assert().success().get_output().stdout.clone();
        let out = String::from_utf8_lossy(&out).to_string();
        if let Some(line) = out.lines().next().filter(|l| !l.trim().is_empty()) {
            break line.split('\t').next().unwrap().to_owned();
        }

        std::thread::sleep(Duration::from_millis(50));
    };

    let mut respond = cargo_bin_cmd!("mm");
    respond.env("MURMUR_DIR", murmur_dir.path());
    respond.args(["question", "respond", &request_id, r#"{"choice":"A"}"#]);
    respond.assert().success().stdout("ok\n");

    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let decision = json["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(decision, "allow");

    let updated = &json["hookSpecificOutput"]["updatedInput"];
    assert_eq!(updated["answers"]["choice"], "A");

    shutdown_daemon(&murmur_dir, daemon);
}

#[tokio::test]
async fn hook_stop_emits_agent_idle_event() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

    add_project(&murmur_dir, &origin);
    let agent_id = create_agent(&murmur_dir);
    assert_eq!(agent_id, "a-1");

    let socket_path = murmur_dir.path().join("murmur.sock");
    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (read_half, write_half) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut writer = tokio::io::BufWriter::new(write_half);

    let attach = murmur_protocol::Request {
        r#type: murmur_protocol::MSG_ATTACH.to_owned(),
        id: "attach-1".to_owned(),
        payload: serde_json::json!({ "projects": [] }),
    };
    write_jsonl(&mut writer, &attach).await.unwrap();

    let resp: murmur_protocol::Response = read_jsonl(&mut reader).await.unwrap().unwrap();
    assert!(resp.success);

    let status = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .env("MURMUR_AGENT_ID", &agent_id)
        .args(["hook", "Stop"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for agent.idle event");
        }

        let evt: murmur_protocol::Event =
            match tokio::time::timeout(Duration::from_secs(1), async {
                read_jsonl(&mut reader).await
            })
            .await
            {
                Ok(Ok(Some(evt))) => evt,
                Ok(Ok(None)) => continue,
                Ok(Err(_)) => continue,
                Err(_) => continue,
            };

        if evt.r#type == murmur_protocol::EVT_AGENT_IDLE {
            assert_eq!(evt.payload["agent_id"], agent_id);
            assert_eq!(evt.payload["project"], "demo");
            break;
        }
    }

    drop(writer);
    drop(reader);
    shutdown_daemon(&murmur_dir, daemon);
}

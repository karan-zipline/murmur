use std::env;
use std::fs;
use std::io::BufRead as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
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

while IFS= read -r line; do
  if [[ -z "${line// }" ]]; then
    continue
  fi
  echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"(fake claude) ok"}]}}'
done
"#,
    );

    write_executable(
        &bin_dir.join("codex"),
        r#"#!/usr/bin/env bash
set -euo pipefail

prompt="${@: -1}"

if [[ "${2:-}" == "resume" ]]; then
  echo '{"type":"item.completed","item":{"id":"i-1","type":"agent_message","text":"(fake codex) resumed: '"$prompt"'"}}'
  echo '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
  exit 0
fi

echo '{"type":"thread.started","thread_id":"t-123"}'
echo '{"type":"item.completed","item":{"id":"i-1","type":"agent_message","text":"(fake codex) new: '"$prompt"'"}}'
echo '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}'
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

#[test]
fn agent_lifecycle_creates_worktree_persists_runtime_and_records_chat() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

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

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["agent", "create", "demo", "ISSUE-1"]);
    let output = create.assert().success().get_output().stdout.clone();
    let agent_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(agent_id, "a-1");

    let repo_dir = murmur_dir.path().join("projects").join("demo").join("repo");
    let worktree_dir = murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-1");
    assert!(worktree_dir.exists());

    let list = Command::new("git")
        .args(["-C", repo_dir.to_str().unwrap(), "worktree", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(
        list_out.contains(worktree_dir.to_str().unwrap()),
        "worktree list did not include worktree dir; output was: {list_out}"
    );

    let agents_json_path = murmur_dir.path().join("runtime").join("agents.json");
    let agents_json = read_to_string_best_effort(&agents_json_path);
    assert!(!agents_json.is_empty(), "agents.json not written");
    let parsed: serde_json::Value = serde_json::from_str(&agents_json).unwrap();
    assert!(parsed.is_array());
    assert!(agents_json.contains("\"id\": \"a-1\""));

    let mut send = cargo_bin_cmd!("mm");
    send.env("MURMUR_DIR", murmur_dir.path());
    send.args(["agent", "send-message", "a-1", "hello"]);
    send.assert().success().stdout("ok\n");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let mut hist = cargo_bin_cmd!("mm");
            hist.env("MURMUR_DIR", murmur_dir.path());
            hist.args(["agent", "chat-history", "a-1", "--limit", "50"]);
            let assert = hist.assert().success();
            let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
            panic!("timed out waiting for claude response; history was:\n{out}");
        }

        let mut hist = cargo_bin_cmd!("mm");
        hist.env("MURMUR_DIR", murmur_dir.path());
        hist.args(["agent", "chat-history", "a-1", "--limit", "50"]);
        let assert = hist.assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        if out.contains("assistant\t(fake claude) ok") {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let mut abort = cargo_bin_cmd!("mm");
    abort.env("MURMUR_DIR", murmur_dir.path());
    abort.args(["agent", "abort", "--yes", "a-1"]);
    abort.assert().success().stdout("ok\n");

    let mut list_agents = cargo_bin_cmd!("mm");
    list_agents.env("MURMUR_DIR", murmur_dir.path());
    list_agents.args(["agent", "list"]);
    list_agents
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "a-1\tdemo\tcoding\taborted\tISSUE-1",
        ));

    let mut delete = cargo_bin_cmd!("mm");
    delete.env("MURMUR_DIR", murmur_dir.path());
    delete.args(["agent", "delete", "a-1"]);
    delete.assert().success().stdout("ok\n");

    assert!(!worktree_dir.exists(), "worktree should be removed");

    let agents_json = read_to_string_best_effort(&agents_json_path);
    let parsed: serde_json::Value = serde_json::from_str(&agents_json).unwrap();
    assert!(parsed.is_array());
    assert!(
        !agents_json.contains("\"id\": \"a-1\""),
        "agents.json should not contain deleted agent; file was:\n{agents_json}"
    );

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn agent_tail_streams_chat_events() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

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

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["agent", "create", "demo", "ISSUE-1"]);
    let output = create.assert().success().get_output().stdout.clone();
    let agent_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(agent_id, "a-1");

    let mut tail = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", murmur_dir.path())
        .args(["agent", "tail", &agent_id])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let stdout = tail.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    std::thread::sleep(Duration::from_millis(100));

    let mut send = cargo_bin_cmd!("mm");
    send.env("MURMUR_DIR", murmur_dir.path());
    send.args(["agent", "send-message", &agent_id, "hello"]);
    send.assert().success().stdout("ok\n");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let pid = Pid::from_raw(tail.id() as i32);
            let _ = kill(pid, Signal::SIGKILL);
            panic!("timed out waiting for agent tail output");
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if line.contains("assistant\t(fake claude) ok") {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let pid = Pid::from_raw(tail.id() as i32);
    let _ = kill(pid, Signal::SIGKILL);
    let _ = tail.wait();

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn agent_codex_backend_resumes_and_records_chat() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&murmur_dir, &bins.path().join("bin"));

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

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["agent", "create", "demo", "ISSUE-1", "--backend", "codex"]);
    let output = create.assert().success().get_output().stdout.clone();
    let agent_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(agent_id, "a-1");

    let mut send1 = cargo_bin_cmd!("mm");
    send1.env("MURMUR_DIR", murmur_dir.path());
    send1.args(["agent", "send-message", &agent_id, "hello"]);
    send1.assert().success().stdout("ok\n");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let mut hist = cargo_bin_cmd!("mm");
            hist.env("MURMUR_DIR", murmur_dir.path());
            hist.args(["agent", "chat-history", &agent_id, "--limit", "50"]);
            let assert = hist.assert().success();
            let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
            panic!("timed out waiting for codex response; history was:\n{out}");
        }

        let mut hist = cargo_bin_cmd!("mm");
        hist.env("MURMUR_DIR", murmur_dir.path());
        hist.args(["agent", "chat-history", &agent_id, "--limit", "50"]);
        let assert = hist.assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        if out.contains("assistant\t(fake codex) new: hello") {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let mut send2 = cargo_bin_cmd!("mm");
    send2.env("MURMUR_DIR", murmur_dir.path());
    send2.args(["agent", "send-message", &agent_id, "second"]);
    send2.assert().success().stdout("ok\n");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let mut hist = cargo_bin_cmd!("mm");
            hist.env("MURMUR_DIR", murmur_dir.path());
            hist.args(["agent", "chat-history", &agent_id, "--limit", "50"]);
            let assert = hist.assert().success();
            let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
            panic!("timed out waiting for codex resume response; history was:\n{out}");
        }

        let mut hist = cargo_bin_cmd!("mm");
        hist.env("MURMUR_DIR", murmur_dir.path());
        hist.args(["agent", "chat-history", &agent_id, "--limit", "50"]);
        let assert = hist.assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
        if out.contains("assistant\t(fake codex) resumed: second") {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    shutdown_daemon(&murmur_dir, daemon);
}

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use predicates::prelude::*;
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
fn plan_start_creates_plan_file_and_can_show_and_stop() {
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

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["plan", "start", "--project", "demo", "test-plan"]);
    let output = start.assert().success().get_output().stdout.clone();
    let plan_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(plan_id, "plan-1");

    let plan_path = murmur_dir.path().join("plans").join("plan-1.md");
    let plan_contents = read_to_string_best_effort(&plan_path);
    assert!(plan_contents.contains("# Plan: plan-1"));
    assert!(plan_contents.contains("test-plan"));

    let mut show = cargo_bin_cmd!("mm");
    show.env("MURMUR_DIR", murmur_dir.path());
    show.args(["plan", "show", &plan_id]);
    show.assert()
        .success()
        .stdout(predicates::str::contains("## Prompt"));

    let mut send = cargo_bin_cmd!("mm");
    send.env("MURMUR_DIR", murmur_dir.path());
    send.args(["plan", "send-message", &plan_id, "hello"]);
    send.assert().success().stdout("ok\n");

    let mut hist = cargo_bin_cmd!("mm");
    hist.env("MURMUR_DIR", murmur_dir.path());
    hist.args(["plan", "chat-history", &plan_id, "--limit", "50"]);
    hist.assert()
        .success()
        .stdout(predicates::str::contains("user\thello"));

    let worktree_dir = murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-plan-1");
    assert!(worktree_dir.exists());

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["plan", "stop", &plan_id]);
    stop.assert().success().stdout("ok\n");

    assert!(!worktree_dir.exists(), "planner worktree should be removed");
    assert!(plan_path.exists(), "plan file should be kept");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn manager_start_send_message_status_and_stop() {
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
        "--backend",
        "claude",
    ]);
    add.assert().success().stdout("ok\n");

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["manager", "start", "demo"]);
    start.assert().success().stdout("ok\n");

    // Wait for the manager agent to transition from starting to running.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for manager agent to reach running state");
        }

        let mut list_agents = cargo_bin_cmd!("mm");
        list_agents.env("MURMUR_DIR", murmur_dir.path());
        list_agents.args(["agent", "list"]);
        let out = list_agents.assert().success().get_output().stdout.clone();
        let s = String::from_utf8_lossy(&out);
        if s.contains("manager-demo\tdemo\tmanager\trunning\tmanager") {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let mut send = cargo_bin_cmd!("mm");
    send.env("MURMUR_DIR", murmur_dir.path());
    send.args(["manager", "send-message", "demo", "hello"]);
    send.assert().success().stdout("ok\n");

    let mut hist = cargo_bin_cmd!("mm");
    hist.env("MURMUR_DIR", murmur_dir.path());
    hist.args(["manager", "chat-history", "demo", "--limit", "50"]);
    hist.assert()
        .success()
        .stdout(predicates::str::contains("user\thello"));

    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", murmur_dir.path());
    status.args(["manager", "status", "demo"]);
    status
        .assert()
        .success()
        .stdout(predicates::str::contains("id\tmanager-demo"));

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["manager", "stop", "demo"]);
    stop.assert().success().stdout("ok\n");

    let mut list_agents = cargo_bin_cmd!("mm");
    list_agents.env("MURMUR_DIR", murmur_dir.path());
    list_agents.args(["agent", "list"]);
    list_agents
        .assert()
        .success()
        .stdout(predicates::str::contains("manager-demo").not());

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn manager_respects_agent_backend_codex() {
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

    let mut set_backend = cargo_bin_cmd!("mm");
    set_backend.env("MURMUR_DIR", murmur_dir.path());
    set_backend.args(["project", "config", "set", "demo", "agent-backend", "codex"]);
    set_backend.assert().success().stdout("ok\n");

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["manager", "start", "demo"]);
    start.assert().success().stdout("ok\n");

    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", murmur_dir.path());
    status.args(["manager", "status", "demo"]);
    status
        .assert()
        .success()
        .stdout(predicates::str::contains("backend\tcodex"));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn manager_can_stop_and_restart_with_existing_branch() {
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

    let mut set_backend = cargo_bin_cmd!("mm");
    set_backend.env("MURMUR_DIR", murmur_dir.path());
    set_backend.args(["project", "config", "set", "demo", "agent-backend", "codex"]);
    set_backend.assert().success().stdout("ok\n");

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["manager", "start", "demo"]);
    start.assert().success().stdout("ok\n");

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["manager", "stop", "demo"]);
    stop.assert().success().stdout("ok\n");

    let mut start_again = cargo_bin_cmd!("mm");
    start_again.env("MURMUR_DIR", murmur_dir.path());
    start_again.args(["manager", "start", "demo"]);
    start_again.assert().success().stdout("ok\n");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn agent_plan_alias_starts_lists_and_stops() {
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

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["agent", "plan", "--project", "demo", "test", "plan"]);
    let output = start.assert().success().get_output().stdout.clone();
    let plan_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(plan_id, "plan-1");

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["agent", "plan", "list", "--project", "demo"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("plan-1\tdemo\t"));

    let worktree_dir = murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-plan-1");
    assert!(worktree_dir.exists());

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["agent", "plan", "stop", &plan_id]);
    stop.assert().success().stdout("ok\n");

    assert!(!worktree_dir.exists(), "planner worktree should be removed");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn project_remove_delete_worktrees_removes_worktrees() {
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

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["plan", "start", "--project", "demo", "test-plan"]);
    let output = start.assert().success().get_output().stdout.clone();
    let plan_id = String::from_utf8_lossy(&output).trim().to_owned();
    assert_eq!(plan_id, "plan-1");

    let repo_dir = murmur_dir.path().join("projects").join("demo").join("repo");
    assert!(repo_dir.join(".git").exists());

    let worktree_dir = murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-plan-1");
    assert!(worktree_dir.exists());

    let mut remove = cargo_bin_cmd!("mm");
    remove.env("MURMUR_DIR", murmur_dir.path());
    remove.args(["project", "remove", "demo", "--delete-worktrees"]);
    remove.assert().success().stdout("ok\n");

    assert!(!worktree_dir.exists(), "worktree dir should be removed");

    let out = Command::new("git")
        .current_dir(&repo_dir)
        .args(["worktree", "list"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("wt-plan-1"),
        "repo should not list removed worktree"
    );

    let plan_path = murmur_dir
        .path()
        .join("plans")
        .join(format!("{plan_id}.md"));
    assert!(plan_path.exists(), "plan file should be kept");

    shutdown_daemon(&murmur_dir, daemon);
}

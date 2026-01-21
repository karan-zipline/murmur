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

fn init_local_remote_with_head_main(base: &Path) -> PathBuf {
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
    run_git(
        base,
        &[
            "-C",
            origin.to_str().unwrap(),
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ],
    );

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

fn spawn_daemon(dir: &TempDir, home: &Path, bin_dir: &Path) -> std::process::Child {
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
        .env("HOME", home)
        .env("PATH", path)
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(dir);
    child
}

fn spawn_daemon_with_merge_delay(
    dir: &TempDir,
    home: &Path,
    bin_dir: &Path,
    delay_ms: u64,
) -> std::process::Child {
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
        .env("HOME", home)
        .env("PATH", path)
        .env("FUGUE_TEST_MERGE_DELAY_MS", delay_ms.to_string())
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

#[test]
fn agent_done_merges_closes_and_records_recent_work() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

    let fugue_dir = TempDir::new().unwrap();
    let home_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&fugue_dir, home_dir.path(), &bins.path().join("bin"));

    let mut add = cargo_bin_cmd!("fugue");
    add.env("FUGUE_DIR", fugue_dir.path());
    add.args([
        "project",
        "add",
        "demo",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add.assert().success().stdout("ok\n");

    let repo_dir = fugue_dir.path().join("projects").join("demo").join("repo");
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut issue = cargo_bin_cmd!("fugue");
    issue.env("FUGUE_DIR", fugue_dir.path());
    issue.args(["issue", "create", "-p", "demo", "Test issue"]);
    let issue_id = String::from_utf8_lossy(&issue.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert!(!issue_id.is_empty());

    let mut create = cargo_bin_cmd!("fugue");
    create.env("FUGUE_DIR", fugue_dir.path());
    create.args(["agent", "create", "demo", &issue_id]);
    let agent_id = String::from_utf8_lossy(&create.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert_eq!(agent_id, "a-1");

    let worktree_dir = fugue_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-1");
    assert!(worktree_dir.exists());

    fs::write(worktree_dir.join("agent.txt"), "from agent\n").unwrap();
    run_git(&worktree_dir, &["add", "."]);
    run_git(&worktree_dir, &["commit", "-m", "agent: add agent.txt"]);

    let mut done = cargo_bin_cmd!("fugue");
    done.env("FUGUE_DIR", fugue_dir.path());
    done.env("FUGUE_AGENT_ID", &agent_id);
    done.args(["agent", "done", "--task", &issue_id]);
    done.assert().success().stdout("ok\n");

    assert!(
        !worktree_dir.exists(),
        "worktree should be removed after done"
    );

    let mut list = cargo_bin_cmd!("fugue");
    list.env("FUGUE_DIR", fugue_dir.path());
    list.args(["agent", "list"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("a-1").not());

    let mut get = cargo_bin_cmd!("fugue");
    get.env("FUGUE_DIR", fugue_dir.path());
    get.args(["issue", "show", "-p", "demo", &issue_id]);
    get.assert()
        .success()
        .stdout(predicates::str::contains("status\tclosed"));

    let inspect = tmp.path().join("inspect");
    run_git(
        tmp.path(),
        &["clone", origin.to_str().unwrap(), inspect.to_str().unwrap()],
    );
    run_git(&inspect, &["checkout", "main"]);
    let agent_txt = fs::read_to_string(inspect.join("agent.txt")).unwrap();
    assert_eq!(agent_txt, "from agent\n");

    let mut commits = cargo_bin_cmd!("fugue");
    commits.env("FUGUE_DIR", fugue_dir.path());
    commits.args(["commit", "list", "--project", "demo", "--limit", "10"]);
    let out = commits.assert().success().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("\ta-1\t"), "commit list was: {s}");
    assert!(
        s.contains(&format!("\t{issue_id}\t")),
        "commit list was: {s}"
    );

    let mut stats = cargo_bin_cmd!("fugue");
    stats.env("FUGUE_DIR", fugue_dir.path());
    stats.args(["stats", "--project", "demo"]);
    stats
        .assert()
        .success()
        .stdout(predicates::str::contains("commit_count\t1"));

    shutdown_daemon(&fugue_dir, daemon);
}

#[test]
fn agent_done_conflict_marks_needs_resolution_and_keeps_worktree() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

    let fugue_dir = TempDir::new().unwrap();
    let home_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(&fugue_dir, home_dir.path(), &bins.path().join("bin"));

    let mut add = cargo_bin_cmd!("fugue");
    add.env("FUGUE_DIR", fugue_dir.path());
    add.args([
        "project",
        "add",
        "demo",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add.assert().success().stdout("ok\n");

    let repo_dir = fugue_dir.path().join("projects").join("demo").join("repo");
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut issue = cargo_bin_cmd!("fugue");
    issue.env("FUGUE_DIR", fugue_dir.path());
    issue.args(["issue", "create", "-p", "demo", "Conflicting issue"]);
    let issue_id = String::from_utf8_lossy(&issue.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut create = cargo_bin_cmd!("fugue");
    create.env("FUGUE_DIR", fugue_dir.path());
    create.args(["agent", "create", "demo", &issue_id]);
    let agent_id = String::from_utf8_lossy(&create.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert_eq!(agent_id, "a-1");

    let worktree_dir = fugue_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-1");
    assert!(worktree_dir.exists());

    fs::write(worktree_dir.join("README.md"), "agent edit\n").unwrap();
    run_git(&worktree_dir, &["add", "README.md"]);
    run_git(&worktree_dir, &["commit", "-m", "agent: edit readme"]);

    let upstream = tmp.path().join("upstream");
    run_git(
        tmp.path(),
        &[
            "clone",
            origin.to_str().unwrap(),
            upstream.to_str().unwrap(),
        ],
    );
    run_git(&upstream, &["checkout", "main"]);
    run_git(&upstream, &["config", "user.name", "Test"]);
    run_git(&upstream, &["config", "user.email", "test@example.com"]);
    fs::write(upstream.join("README.md"), "upstream edit\n").unwrap();
    run_git(&upstream, &["add", "README.md"]);
    run_git(&upstream, &["commit", "-m", "upstream: edit readme"]);
    run_git(&upstream, &["push", "origin", "main"]);

    let mut done = cargo_bin_cmd!("fugue");
    done.env("FUGUE_DIR", fugue_dir.path());
    done.env("FUGUE_AGENT_ID", &agent_id);
    done.args(["agent", "done"]);
    done.assert().failure();

    assert!(worktree_dir.exists(), "worktree should remain on conflict");

    let mut list = cargo_bin_cmd!("fugue");
    list.env("FUGUE_DIR", fugue_dir.path());
    list.args(["agent", "list"]);
    list.assert().success().stdout(predicates::str::contains(
        "a-1\tdemo\tcoding\tneeds_resolution\t",
    ));

    let mut get = cargo_bin_cmd!("fugue");
    get.env("FUGUE_DIR", fugue_dir.path());
    get.args(["issue", "show", "-p", "demo", &issue_id]);
    get.assert()
        .success()
        .stdout(predicates::str::contains("status\topen"));

    shutdown_daemon(&fugue_dir, daemon);
}

#[test]
fn concurrent_agent_done_is_serialized_per_project() {
    use std::sync::{Arc, Barrier};

    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

    let fugue_dir = TempDir::new().unwrap();
    let home_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon =
        spawn_daemon_with_merge_delay(&fugue_dir, home_dir.path(), &bins.path().join("bin"), 250);

    let mut add = cargo_bin_cmd!("fugue");
    add.env("FUGUE_DIR", fugue_dir.path());
    add.args([
        "project",
        "add",
        "demo",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add.assert().success().stdout("ok\n");

    let repo_dir = fugue_dir.path().join("projects").join("demo").join("repo");
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut issue1 = cargo_bin_cmd!("fugue");
    issue1.env("FUGUE_DIR", fugue_dir.path());
    issue1.args(["issue", "create", "-p", "demo", "Issue one"]);
    let issue_id_1 = String::from_utf8_lossy(&issue1.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut issue2 = cargo_bin_cmd!("fugue");
    issue2.env("FUGUE_DIR", fugue_dir.path());
    issue2.args(["issue", "create", "-p", "demo", "Issue two"]);
    let issue_id_2 = String::from_utf8_lossy(&issue2.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut create1 = cargo_bin_cmd!("fugue");
    create1.env("FUGUE_DIR", fugue_dir.path());
    create1.args(["agent", "create", "demo", &issue_id_1]);
    let agent_1 = String::from_utf8_lossy(&create1.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert_eq!(agent_1, "a-1");

    let mut create2 = cargo_bin_cmd!("fugue");
    create2.env("FUGUE_DIR", fugue_dir.path());
    create2.args(["agent", "create", "demo", &issue_id_2]);
    let agent_2 = String::from_utf8_lossy(&create2.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert_eq!(agent_2, "a-2");

    let wt1 = fugue_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-1");
    let wt2 = fugue_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-2");
    assert!(wt1.exists());
    assert!(wt2.exists());

    fs::write(wt1.join("a1.txt"), "a1\n").unwrap();
    run_git(&wt1, &["add", "."]);
    run_git(&wt1, &["commit", "-m", "agent1"]);

    fs::write(wt2.join("a2.txt"), "a2\n").unwrap();
    run_git(&wt2, &["add", "."]);
    run_git(&wt2, &["commit", "-m", "agent2"]);

    let barrier = Arc::new(Barrier::new(3));
    let fugue_dir_path = fugue_dir.path().to_path_buf();

    let b1 = barrier.clone();
    let d1 = fugue_dir_path.clone();
    let a1 = agent_1.clone();
    let t1 = std::thread::spawn(move || {
        b1.wait();
        let mut done = cargo_bin_cmd!("fugue");
        done.env("FUGUE_DIR", &d1);
        done.env("FUGUE_AGENT_ID", &a1);
        done.args(["agent", "done"]);
        done.assert().success().stdout("ok\n");
    });

    let b2 = barrier.clone();
    let d2 = fugue_dir_path.clone();
    let a2 = agent_2.clone();
    let t2 = std::thread::spawn(move || {
        b2.wait();
        let mut done = cargo_bin_cmd!("fugue");
        done.env("FUGUE_DIR", &d2);
        done.env("FUGUE_AGENT_ID", &a2);
        done.args(["agent", "done"]);
        done.assert().success().stdout("ok\n");
    });

    barrier.wait();
    t1.join().unwrap();
    t2.join().unwrap();

    let inspect = tmp.path().join("inspect2");
    run_git(
        tmp.path(),
        &["clone", origin.to_str().unwrap(), inspect.to_str().unwrap()],
    );
    run_git(&inspect, &["checkout", "main"]);
    assert_eq!(fs::read_to_string(inspect.join("a1.txt")).unwrap(), "a1\n");
    assert_eq!(fs::read_to_string(inspect.join("a2.txt")).unwrap(), "a2\n");

    let mut stats = cargo_bin_cmd!("fugue");
    stats.env("FUGUE_DIR", fugue_dir.path());
    stats.args(["stats", "--project", "demo"]);
    stats
        .assert()
        .success()
        .stdout(predicates::str::contains("commit_count\t2"));

    shutdown_daemon(&fugue_dir, daemon);
}

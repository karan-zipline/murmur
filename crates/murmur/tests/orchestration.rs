use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
        .env("FUGUE_ORCHESTRATOR_INTERVAL_MS", "50")
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

fn create_tk_issue(murmur_dir: &TempDir, project: &str, title: &str) -> String {
    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["issue", "create", "-p", project, title]);
    let out = create.assert().success().get_output().stdout.clone();
    String::from_utf8_lossy(&out).trim().to_owned()
}

fn claim_list_lines(murmur_dir: &TempDir, project: &str) -> Vec<(String, String)> {
    let mut cmd = cargo_bin_cmd!("mm");
    cmd.env("MURMUR_DIR", murmur_dir.path());
    cmd.args(["claim", "list", "--project", project]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let s = String::from_utf8_lossy(&out);
    s.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let proj = parts.next()?;
            let issue_id = parts.next()?;
            let agent_id = parts.next()?;
            if proj != project {
                return None;
            }
            Some((issue_id.to_owned(), agent_id.to_owned()))
        })
        .collect()
}

fn wait_for_single_claim(murmur_dir: &TempDir, project: &str) -> (String, String) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            panic!("timed out waiting for claim.list to return one claim");
        }

        let claims = claim_list_lines(murmur_dir, project);
        if claims.len() == 1 {
            return claims[0].clone();
        }

        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn orchestration_spawns_next_issue_after_agent_done() {
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
        "--max-agents",
        "1",
    ]);
    add.assert().success().stdout("ok\n");

    let issue_a = create_tk_issue(&murmur_dir, "demo", "First issue");
    let issue_b = create_tk_issue(&murmur_dir, "demo", "Second issue");
    assert!(!issue_a.is_empty());
    assert!(!issue_b.is_empty());
    assert_ne!(issue_a, issue_b);

    let mut status0 = cargo_bin_cmd!("mm");
    status0.env("MURMUR_DIR", murmur_dir.path());
    status0.args(["orchestration", "status", "demo"]);
    status0
        .assert()
        .success()
        .stdout(predicates::str::contains("running\tfalse"));

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["orchestration", "start", "demo"]);
    start.assert().success().stdout("ok\n");

    let (claimed_issue_1, agent_1) = wait_for_single_claim(&murmur_dir, "demo");
    assert_eq!(agent_1, "a-1");
    assert!(claimed_issue_1 == issue_a || claimed_issue_1 == issue_b);

    let mut done = cargo_bin_cmd!("mm");
    done.env("MURMUR_DIR", murmur_dir.path());
    done.env("MURMUR_AGENT_ID", &agent_1);
    done.args(["agent", "done"]);
    done.assert().success().stdout("ok\n");

    let (claimed_issue_2, agent_2) = wait_for_single_claim(&murmur_dir, "demo");
    assert_eq!(agent_2, "a-2");
    assert_ne!(claimed_issue_2, claimed_issue_1);
    assert!(claimed_issue_2 == issue_a || claimed_issue_2 == issue_b);

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["orchestration", "stop", "demo"]);
    stop.assert().success().stdout("ok\n");

    let mut status1 = cargo_bin_cmd!("mm");
    status1.env("MURMUR_DIR", murmur_dir.path());
    status1.args(["orchestration", "status", "demo"]);
    status1
        .assert()
        .success()
        .stdout(predicates::str::contains("running\tfalse"));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn orchestration_stop_can_abort_active_agents() {
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
        "--max-agents",
        "1",
    ]);
    add.assert().success().stdout("ok\n");

    let _issue_a = create_tk_issue(&murmur_dir, "demo", "First issue");
    let _issue_b = create_tk_issue(&murmur_dir, "demo", "Second issue");

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["orchestration", "start", "demo"]);
    start.assert().success().stdout("ok\n");

    let (_issue_id, agent_id) = wait_for_single_claim(&murmur_dir, "demo");

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["orchestration", "stop", "demo", "--abort-agents"]);
    stop.assert().success().stdout("ok\n");

    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", murmur_dir.path());
    status.args(["orchestration", "status", "demo"]);
    status
        .assert()
        .success()
        .stdout(predicates::str::contains("running\tfalse"));

    let claims = claim_list_lines(&murmur_dir, "demo");
    assert!(
        claims.is_empty(),
        "expected no claims after aborting agent; got {claims:?}"
    );

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["agent", "list", "--project", "demo"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains(format!("{agent_id}\tdemo")))
        .stdout(predicates::str::contains("\taborted\t"));

    shutdown_daemon(&murmur_dir, daemon);
}

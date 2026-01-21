use std::fs;
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

fn spawn_daemon(dir: &TempDir) -> std::process::Child {
    let child = Command::new(assert_cmd::cargo::cargo_bin!("mm"))
        .env("MURMUR_DIR", dir.path())
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

fn orchestration_status_contains(dir: &TempDir, project: &str, expected_running: bool) {
    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", dir.path());
    status.args(["orchestration", "status", project]);
    status
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "running\t{expected_running}"
        )));
}

#[test]
fn project_start_stop_all_controls_orchestration() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let daemon = spawn_daemon(&murmur_dir);

    for name in ["demo-a", "demo-b"] {
        let mut add = cargo_bin_cmd!("mm");
        add.env("MURMUR_DIR", murmur_dir.path());
        add.args([
            "project",
            "add",
            name,
            "--remote-url",
            origin.to_str().unwrap(),
        ]);
        add.assert().success().stdout("ok\n");

        orchestration_status_contains(&murmur_dir, name, false);
    }

    let mut start = cargo_bin_cmd!("mm");
    start.env("MURMUR_DIR", murmur_dir.path());
    start.args(["project", "start", "--all"]);
    start.assert().success().stdout("ok\n");

    for name in ["demo-a", "demo-b"] {
        orchestration_status_contains(&murmur_dir, name, true);
    }

    let mut stop = cargo_bin_cmd!("mm");
    stop.env("MURMUR_DIR", murmur_dir.path());
    stop.args(["project", "stop", "--all"]);
    stop.assert().success().stdout("ok\n");

    for name in ["demo-a", "demo-b"] {
        orchestration_status_contains(&murmur_dir, name, false);
    }

    shutdown_daemon(&murmur_dir, daemon);
}

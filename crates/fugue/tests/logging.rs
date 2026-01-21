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

#[test]
fn creates_log_file_for_trivial_command() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("fugue.log");

    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.env("FUGUE_DIR", dir.path());
    cmd.args(["server", "status"]);

    cmd.assert().success();

    let log = read_to_string_best_effort(&log_path);
    assert!(log.contains("fugue starting"), "log was: {log}");
}

#[test]
fn server_start_foreground_handles_sigint_and_logs_ready() {
    let dir = TempDir::new().unwrap();
    let log_path = dir.path().join("fugue.log");

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("fugue"))
        .env("FUGUE_DIR", dir.path())
        .args(["server", "start", "--foreground"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if Instant::now() > deadline {
            let log = read_to_string_best_effort(&log_path);
            panic!("timed out waiting for daemon ready; log was: {log}");
        }

        let log = read_to_string_best_effort(&log_path);
        if log.contains("daemon ready") {
            break;
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    let pid = Pid::from_raw(child.id() as i32);
    kill(pid, Signal::SIGINT).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "status: {status:?}");
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("timed out waiting for daemon to exit");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let log = read_to_string_best_effort(&log_path);
    assert!(log.contains("daemon starting"), "log was: {log}");
    assert!(log.contains("daemon ready"), "log was: {log}");
    assert!(log.contains("daemon shutting down"), "log was: {log}");
}

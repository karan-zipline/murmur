use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tempfile::TempDir;

fn read_to_string_best_effort(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
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

fn shutdown_daemon(dir: &TempDir, mut child: std::process::Child) {
    let mut shutdown = cargo_bin_cmd!("fugue");
    shutdown.args([
        "--fugue-dir",
        dir.path().to_str().unwrap(),
        "server",
        "shutdown",
    ]);
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
fn fugue_dir_flag_overrides_base_directory() {
    let dir = TempDir::new().unwrap();

    let child = Command::new(assert_cmd::cargo::cargo_bin!("fugue"))
        .args([
            "--fugue-dir",
            dir.path().to_str().unwrap(),
            "server",
            "start",
            "--foreground",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_daemon_ready(&dir);
    assert!(dir.path().join("fugue.sock").exists());
    assert!(dir.path().join("fugue.log").exists());

    shutdown_daemon(&dir, child);
}

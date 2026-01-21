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

#[test]
fn project_add_list_config_and_remove() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let daemon = spawn_daemon(&murmur_dir);

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

    let repo_dir = murmur_dir.path().join("projects").join("demo").join("repo");
    assert!(repo_dir.join(".git").exists());

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["project", "list"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("demo\t"));

    let cfg_path = murmur_dir.path().join("config").join("config.toml");
    let cfg = read_to_string_best_effort(&cfg_path);
    assert!(cfg.contains("[[projects]]"));
    assert!(cfg.contains("name = \"demo\""));
    assert!(cfg.contains("remote-url ="));

    let mut get = cargo_bin_cmd!("mm");
    get.env("MURMUR_DIR", murmur_dir.path());
    get.args(["project", "config", "get", "demo", "max-agents"]);
    get.assert().success().stdout("3\n");

    let mut set = cargo_bin_cmd!("mm");
    set.env("MURMUR_DIR", murmur_dir.path());
    set.args(["project", "config", "set", "demo", "max-agents", "5"]);
    set.assert().success().stdout("ok\n");

    let mut get2 = cargo_bin_cmd!("mm");
    get2.env("MURMUR_DIR", murmur_dir.path());
    get2.args(["project", "config", "get", "demo", "max-agents"]);
    get2.assert().success().stdout("5\n");

    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", murmur_dir.path());
    status.args(["project", "status", "demo"]);
    status
        .assert()
        .success()
        .stdout(predicates::str::contains("repo_exists\ttrue"))
        .stdout(predicates::str::contains("remote_matches\ttrue"));

    let mut remove = cargo_bin_cmd!("mm");
    remove.env("MURMUR_DIR", murmur_dir.path());
    remove.args(["project", "remove", "demo"]);
    remove.assert().success().stdout("ok\n");

    let mut list2 = cargo_bin_cmd!("mm");
    list2.env("MURMUR_DIR", murmur_dir.path());
    list2.args(["project", "list"]);
    list2
        .assert()
        .success()
        .stdout(predicates::str::contains("demo\t").not());

    assert!(repo_dir.join(".git").exists(), "repo should remain on disk");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn project_add_reuses_existing_repo_dir_after_remove() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote(tmp.path());

    let murmur_dir = TempDir::new().unwrap();
    let daemon = spawn_daemon(&murmur_dir);

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

    let repo_dir = murmur_dir.path().join("projects").join("demo").join("repo");

    let mut remove = cargo_bin_cmd!("mm");
    remove.env("MURMUR_DIR", murmur_dir.path());
    remove.args(["project", "remove", "demo"]);
    remove.assert().success().stdout("ok\n");

    assert!(repo_dir.join(".git").exists(), "repo should remain on disk");

    let mut add_again = cargo_bin_cmd!("mm");
    add_again.env("MURMUR_DIR", murmur_dir.path());
    add_again.args([
        "project",
        "add",
        "demo",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add_again.assert().success().stdout("ok\n");

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["project", "list"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("demo\t"));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn daemon_loads_existing_config() {
    let murmur_dir = TempDir::new().unwrap();
    let cfg_dir = murmur_dir.path().join("config");
    fs::create_dir_all(&cfg_dir).unwrap();

    fs::write(
        cfg_dir.join("config.toml"),
        r#"
[[projects]]
name = "demo"
remote-url = "file:///tmp/demo.git"
"#,
    )
    .unwrap();

    let daemon = spawn_daemon(&murmur_dir);

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["project", "list"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("demo\tfile:///tmp/demo.git"));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn project_add_accepts_local_repo_path_and_infers_name() {
    let tmp = TempDir::new().unwrap();
    let _origin = init_local_remote(tmp.path());
    let seed = tmp.path().join("seed");

    let murmur_dir = TempDir::new().unwrap();
    let daemon = spawn_daemon(&murmur_dir);

    let mut add = cargo_bin_cmd!("mm");
    add.env("MURMUR_DIR", murmur_dir.path());
    add.args(["project", "add", seed.to_str().unwrap()]);
    add.assert().success().stdout("ok\n");

    let repo_dir = murmur_dir.path().join("projects").join("seed").join("repo");
    assert!(repo_dir.join(".git").exists());

    let mut list = cargo_bin_cmd!("mm");
    list.env("MURMUR_DIR", murmur_dir.path());
    list.args(["project", "list"]);
    list.assert()
        .success()
        .stdout(predicates::str::contains("seed\t"));

    shutdown_daemon(&murmur_dir, daemon);
}

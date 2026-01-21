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
fn tk_issue_ready_close_comment_and_commit_work() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut create_a = cargo_bin_cmd!("mm");
    create_a.env("MURMUR_DIR", murmur_dir.path());
    create_a.args(["issue", "create", "-p", "demo", "First issue"]);
    let out = create_a.assert().success().get_output().stdout.clone();
    let issue_a = String::from_utf8_lossy(&out).trim().to_owned();
    assert!(!issue_a.is_empty());

    let mut create_b = cargo_bin_cmd!("mm");
    create_b.env("MURMUR_DIR", murmur_dir.path());
    create_b.args([
        "issue",
        "create",
        "-p",
        "demo",
        "Second issue",
        "--depends-on",
        &issue_a,
    ]);
    let out = create_b.assert().success().get_output().stdout.clone();
    let issue_b = String::from_utf8_lossy(&out).trim().to_owned();
    assert_ne!(issue_b, issue_a);

    let mut ready = cargo_bin_cmd!("mm");
    ready.env("MURMUR_DIR", murmur_dir.path());
    ready.args(["issue", "ready", "-p", "demo"]);
    ready
        .assert()
        .success()
        .stdout(predicates::str::contains(&issue_a))
        .stdout(predicates::str::contains(&issue_b).not());

    let mut comment = cargo_bin_cmd!("mm");
    comment.env("MURMUR_DIR", murmur_dir.path());
    comment.args([
        "issue", "comment", "-p", "demo", &issue_a, "--body", "hello",
    ]);
    comment.assert().success().stdout("ok\n");

    let mut get = cargo_bin_cmd!("mm");
    get.env("MURMUR_DIR", murmur_dir.path());
    get.args(["issue", "show", "-p", "demo", &issue_a]);
    get.assert()
        .success()
        .stdout(predicates::str::contains("## Comments"))
        .stdout(predicates::str::contains("hello"));

    let mut close = cargo_bin_cmd!("mm");
    close.env("MURMUR_DIR", murmur_dir.path());
    close.args(["issue", "close", "-p", "demo", &issue_a]);
    close.assert().success().stdout("ok\n");

    let mut ready2 = cargo_bin_cmd!("mm");
    ready2.env("MURMUR_DIR", murmur_dir.path());
    ready2.args(["issue", "ready", "-p", "demo"]);
    ready2
        .assert()
        .success()
        .stdout(predicates::str::contains(&issue_a).not());

    let mut commit = cargo_bin_cmd!("mm");
    commit.env("MURMUR_DIR", murmur_dir.path());
    commit.args(["issue", "commit", "-p", "demo"]);
    commit.assert().success().stdout("ok\n");

    let log = Command::new("git")
        .args([
            "-C",
            origin.to_str().unwrap(),
            "log",
            "-1",
            "--format=%s",
            "main",
        ])
        .output()
        .unwrap();
    assert!(log.status.success(), "git log failed");
    let subject = String::from_utf8_lossy(&log.stdout).trim().to_owned();
    assert_eq!(subject, "issue: update tickets");

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn tk_issue_project_is_detected_from_repo_cwd() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.current_dir(&repo_dir);
    create.args(["issue", "create", "Inferred project issue"]);
    let issue_id = String::from_utf8_lossy(&create.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert!(!issue_id.is_empty());

    let mut show = cargo_bin_cmd!("mm");
    show.env("MURMUR_DIR", murmur_dir.path());
    show.current_dir(&repo_dir);
    show.args(["issue", "show", &issue_id]);
    show.assert()
        .success()
        .stdout(predicates::str::contains("id\t"))
        .stdout(predicates::str::contains(&issue_id));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn tk_issue_list_filters_by_status() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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

    let mut create_a = cargo_bin_cmd!("mm");
    create_a.env("MURMUR_DIR", murmur_dir.path());
    create_a.args(["issue", "create", "-p", "demo", "Open issue"]);
    let issue_a = String::from_utf8_lossy(&create_a.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut create_b = cargo_bin_cmd!("mm");
    create_b.env("MURMUR_DIR", murmur_dir.path());
    create_b.args(["issue", "create", "-p", "demo", "Closed issue"]);
    let issue_b = String::from_utf8_lossy(&create_b.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut close = cargo_bin_cmd!("mm");
    close.env("MURMUR_DIR", murmur_dir.path());
    close.args(["issue", "close", "-p", "demo", &issue_b]);
    close.assert().success().stdout("ok\n");

    let mut open = cargo_bin_cmd!("mm");
    open.env("MURMUR_DIR", murmur_dir.path());
    open.args(["issue", "list", "-p", "demo", "--status", "open"]);
    open.assert()
        .success()
        .stdout(predicates::str::contains(&issue_a))
        .stdout(predicates::str::contains(&issue_b).not());

    let mut closed = cargo_bin_cmd!("mm");
    closed.env("MURMUR_DIR", murmur_dir.path());
    closed.args(["issue", "list", "-p", "demo", "--status", "closed"]);
    closed
        .assert()
        .success()
        .stdout(predicates::str::contains(&issue_b))
        .stdout(predicates::str::contains(&issue_a).not());

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn tk_issue_create_parent_and_depends_on_are_recorded() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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

    let mut parent = cargo_bin_cmd!("mm");
    parent.env("MURMUR_DIR", murmur_dir.path());
    parent.args(["issue", "create", "-p", "demo", "Parent"]);
    let parent_id = String::from_utf8_lossy(&parent.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut dep = cargo_bin_cmd!("mm");
    dep.env("MURMUR_DIR", murmur_dir.path());
    dep.args(["issue", "create", "-p", "demo", "Dependency"]);
    let dep_id = String::from_utf8_lossy(&dep.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut child = cargo_bin_cmd!("mm");
    child.env("MURMUR_DIR", murmur_dir.path());
    child.args([
        "issue",
        "create",
        "-p",
        "demo",
        "Child",
        "--parent",
        &parent_id,
        "--depends-on",
        &dep_id,
    ]);
    let child_id = String::from_utf8_lossy(&child.assert().success().get_output().stdout)
        .trim()
        .to_owned();

    let mut show = cargo_bin_cmd!("mm");
    show.env("MURMUR_DIR", murmur_dir.path());
    show.args(["issue", "show", "-p", "demo", &child_id]);
    show.assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "deps\t{parent_id},{dep_id}"
        )));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn tk_issue_create_requires_existing_parent() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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

    let mut child = cargo_bin_cmd!("mm");
    child.env("MURMUR_DIR", murmur_dir.path());
    child.args([
        "issue",
        "create",
        "-p",
        "demo",
        "Child",
        "--parent",
        "issue-does-not-exist",
    ]);
    child
        .assert()
        .failure()
        .stderr(predicates::str::contains("validate --parent"));

    shutdown_daemon(&murmur_dir, daemon);
}

#[test]
fn tk_issue_create_commit_flag_pushes() {
    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

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
    run_git(&repo_dir, &["config", "user.name", "Test"]);
    run_git(&repo_dir, &["config", "user.email", "test@example.com"]);

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args([
        "issue",
        "create",
        "-p",
        "demo",
        "Committed issue",
        "--commit",
    ]);
    create.assert().success();

    let log = Command::new("git")
        .args([
            "-C",
            origin.to_str().unwrap(),
            "log",
            "-1",
            "--format=%s",
            "main",
        ])
        .output()
        .unwrap();
    assert!(log.status.success(), "git log failed");
    let subject = String::from_utf8_lossy(&log.stdout).trim().to_owned();
    assert_eq!(subject, "issue: update tickets");

    shutdown_daemon(&murmur_dir, daemon);
}

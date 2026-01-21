use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use serde_json::json;
use tempfile::TempDir;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn spawn_daemon(dir: &TempDir, linear_key: &str, linear_url: &str) -> std::process::Child {
    let child = Command::new(assert_cmd::cargo::cargo_bin!("fugue"))
        .env("FUGUE_DIR", dir.path())
        .env("LINEAR_API_KEY", linear_key)
        .env("LINEAR_GRAPHQL_URL", linear_url)
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

#[tokio::test]
async fn backend_selection_routes_tk_and_linear() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "lin-key"))
        .and(body_string_contains("query Issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issues": {
                    "nodes": [
                        {
                            "identifier": "LIN-1",
                            "title": "Linear ready",
                            "description": "",
                            "priority": 3,
                            "createdAt": "2026-01-20T00:00:00Z",
                            "state": { "type": "backlog" },
                            "labels": { "nodes": [] },
                            "parent": null
                        }
                    ]
                }
            }
        })))
        .mount(&server)
        .await;

    let tmp_repo = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp_repo.path());

    let fugue_dir = TempDir::new().unwrap();
    let daemon = spawn_daemon(&fugue_dir, "lin-key", &server.uri());

    let mut add_tk = cargo_bin_cmd!("fugue");
    add_tk.env("FUGUE_DIR", fugue_dir.path());
    add_tk.args([
        "project",
        "add",
        "tkproj",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add_tk.assert().success().stdout("ok\n");

    let mut add_lin = cargo_bin_cmd!("fugue");
    add_lin.env("FUGUE_DIR", fugue_dir.path());
    add_lin.args([
        "project",
        "add",
        "linproj",
        "--remote-url",
        origin.to_str().unwrap(),
    ]);
    add_lin.assert().success().stdout("ok\n");

    let repo_tk = fugue_dir
        .path()
        .join("projects")
        .join("tkproj")
        .join("repo");
    run_git(&repo_tk, &["config", "user.name", "Test"]);
    run_git(&repo_tk, &["config", "user.email", "test@example.com"]);

    let repo_lin = fugue_dir
        .path()
        .join("projects")
        .join("linproj")
        .join("repo");
    run_git(&repo_lin, &["config", "user.name", "Test"]);
    run_git(&repo_lin, &["config", "user.email", "test@example.com"]);

    let mut set_team = cargo_bin_cmd!("fugue");
    set_team.env("FUGUE_DIR", fugue_dir.path());
    set_team.args([
        "project",
        "config",
        "set",
        "linproj",
        "linear-team",
        "team-1",
    ]);
    set_team.assert().success().stdout("ok\n");

    let mut set_backend = cargo_bin_cmd!("fugue");
    set_backend.env("FUGUE_DIR", fugue_dir.path());
    set_backend.args([
        "project",
        "config",
        "set",
        "linproj",
        "issue-backend",
        "linear",
    ]);
    set_backend.assert().success().stdout("ok\n");

    let mut create = cargo_bin_cmd!("fugue");
    create.env("FUGUE_DIR", fugue_dir.path());
    create.args(["issue", "create", "-p", "tkproj", "Tk issue"]);
    let out = create.assert().success().get_output().stdout.clone();
    let tk_issue = String::from_utf8_lossy(&out).trim().to_owned();
    assert!(!tk_issue.is_empty());

    let mut ready_tk = cargo_bin_cmd!("fugue");
    ready_tk.env("FUGUE_DIR", fugue_dir.path());
    ready_tk.args(["issue", "ready", "-p", "tkproj"]);
    ready_tk
        .assert()
        .success()
        .stdout(predicates::str::contains(&tk_issue));

    let mut ready_lin = cargo_bin_cmd!("fugue");
    ready_lin.env("FUGUE_DIR", fugue_dir.path());
    ready_lin.env("LINEAR_API_KEY", "lin-key");
    ready_lin.env("LINEAR_GRAPHQL_URL", server.uri());
    ready_lin.args(["issue", "ready", "-p", "linproj"]);
    ready_lin
        .assert()
        .success()
        .stdout(predicates::str::contains("LIN-1\tLinear ready"));

    shutdown_daemon(&fugue_dir, daemon);
}

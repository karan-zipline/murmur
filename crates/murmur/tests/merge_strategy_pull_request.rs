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

fn write_git_url_rewrite_config(dir: &TempDir, remote_url: &str, origin: &Path) -> PathBuf {
    let cfg = dir.path().join("gitconfig");
    let origin_url = format!("file://{}", origin.to_string_lossy());
    let contents = format!(
        "[url \"{origin_url}\"]\n\tinsteadOf = {remote_url}\n",
        origin_url = origin_url,
        remote_url = remote_url
    );
    fs::write(&cfg, contents).unwrap();
    cfg
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

fn spawn_daemon(
    dir: &TempDir,
    home: &Path,
    bin_dir: &Path,
    git_config_global: &Path,
    github_token: &str,
    github_graphql_url: &str,
) -> std::process::Child {
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
        .env("HOME", home)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", git_config_global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GITHUB_TOKEN", github_token)
        .env("GITHUB_GRAPHQL_URL", github_graphql_url)
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

#[tokio::test]
async fn agent_done_pull_request_creates_pr_and_keeps_worktree() {
    let github_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("GetRepositoryID"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "repository": { "id": "repo-123" } }
        })))
        .expect(1)
        .mount(&github_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("authorization", "Bearer test-token"))
        .and(body_string_contains("CreatePullRequest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "createPullRequest": { "pullRequest": { "url": "https://github.com/owner/repo/pull/1" } } }
        })))
        .expect(1)
        .mount(&github_server)
        .await;

    let tmp = TempDir::new().unwrap();
    let origin = init_local_remote_with_head_main(tmp.path());

    let remote_url = "https://github.com/owner/repo.git";
    let git_cfg_dir = TempDir::new().unwrap();
    let git_cfg = write_git_url_rewrite_config(&git_cfg_dir, remote_url, &origin);

    let murmur_dir = TempDir::new().unwrap();
    let home_dir = TempDir::new().unwrap();
    let bins = setup_fake_binaries();
    let daemon = spawn_daemon(
        &murmur_dir,
        home_dir.path(),
        &bins.path().join("bin"),
        &git_cfg,
        "test-token",
        &github_server.uri(),
    );

    let mut add = cargo_bin_cmd!("mm");
    add.env("MURMUR_DIR", murmur_dir.path());
    add.args(["project", "add", "demo", "--remote-url", remote_url]);
    add.assert().success().stdout("ok\n");

    let mut set = cargo_bin_cmd!("mm");
    set.env("MURMUR_DIR", murmur_dir.path());
    set.args([
        "project",
        "config",
        "set",
        "demo",
        "merge-strategy",
        "pull-request",
    ]);
    set.assert().success().stdout("ok\n");

    let mut create = cargo_bin_cmd!("mm");
    create.env("MURMUR_DIR", murmur_dir.path());
    create.args(["agent", "create", "demo", "123"]);
    let agent_id = String::from_utf8_lossy(&create.assert().success().get_output().stdout)
        .trim()
        .to_owned();
    assert_eq!(agent_id, "a-1");

    let worktree_dir = murmur_dir
        .path()
        .join("projects")
        .join("demo")
        .join("worktrees")
        .join("wt-a-1");
    assert!(worktree_dir.exists());
    run_git(&worktree_dir, &["config", "user.name", "Test"]);
    run_git(&worktree_dir, &["config", "user.email", "test@example.com"]);

    fs::write(worktree_dir.join("agent.txt"), "from agent\n").unwrap();
    run_git(&worktree_dir, &["add", "."]);
    run_git(&worktree_dir, &["commit", "-m", "agent: add agent.txt"]);

    let mut done = cargo_bin_cmd!("mm");
    done.env("MURMUR_DIR", murmur_dir.path());
    done.env("MURMUR_AGENT_ID", &agent_id);
    done.args(["agent", "done"]);
    let output = done.output().unwrap();
    if !output.status.success() {
        let mut chat = cargo_bin_cmd!("mm");
        chat.env("MURMUR_DIR", murmur_dir.path());
        chat.args(["agent", "chat-history", &agent_id, "--limit", "50"]);
        let chat_out = chat.output().unwrap();
        let log = read_to_string_best_effort(&murmur_dir.path().join("murmur.log"));
        panic!(
            "agent done failed.\nstdout:\n{}\nstderr:\n{}\nchat:\n{}\nlog:\n{log}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&chat_out.stdout),
        );
    }
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");

    assert!(
        worktree_dir.exists(),
        "worktree should be kept for PR strategy"
    );

    let mut list_agents = cargo_bin_cmd!("mm");
    list_agents.env("MURMUR_DIR", murmur_dir.path());
    list_agents.args(["agent", "list"]);
    list_agents
        .assert()
        .success()
        .stdout(predicates::str::contains("a-1\tdemo\tcoding\texited\t123"));

    let mut status = cargo_bin_cmd!("mm");
    status.env("MURMUR_DIR", murmur_dir.path());
    status.args(["status", "--agents"]);
    status.assert().success().stdout(predicates::str::contains(
        "PR: https://github.com/owner/repo/pull/1",
    ));

    let mut claims = cargo_bin_cmd!("mm");
    claims.env("MURMUR_DIR", murmur_dir.path());
    claims.args(["claims", "--project", "demo"]);
    claims
        .assert()
        .success()
        .stdout(predicates::str::contains("123").not());

    let inspect = tmp.path().join("inspect");
    run_git(
        tmp.path(),
        &["clone", origin.to_str().unwrap(), inspect.to_str().unwrap()],
    );
    run_git(&inspect, &["checkout", "main"]);
    assert!(
        !inspect.join("agent.txt").exists(),
        "main should not contain agent changes in PR strategy"
    );
    run_git(&inspect, &["fetch", "origin"]);
    let branches = Command::new("git")
        .current_dir(&inspect)
        .args(["branch", "-a"])
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&branches.stdout);
    assert!(
        branches.contains("remotes/origin/murmur/a-1"),
        "branches were: {branches}"
    );

    shutdown_daemon(&murmur_dir, daemon);
}

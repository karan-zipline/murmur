use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

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
    run_git(&seed, &["config", "user.name", "Test"]);
    run_git(&seed, &["config", "user.email", "test@example.com"]);
    fs::write(seed.join("README.md"), "hello\n").unwrap();
    run_git(&seed, &["add", "."]);
    run_git(&seed, &["commit", "-m", "init"]);
    run_git(&seed, &["push", "-u", "origin", "main"]);

    origin
}

#[test]
fn branch_cleanup_deletes_only_merged_remote_branches() {
    let tmp = TempDir::new().unwrap();
    let _origin = init_local_remote(tmp.path());
    let seed = tmp.path().join("seed");

    run_git(&seed, &["checkout", "-b", "fugue/a-merged"]);
    fs::write(seed.join("merged.txt"), "merged\n").unwrap();
    run_git(&seed, &["add", "."]);
    run_git(&seed, &["commit", "-m", "merged"]);
    run_git(&seed, &["push", "-u", "origin", "fugue/a-merged"]);

    run_git(&seed, &["checkout", "main"]);
    run_git(
        &seed,
        &["merge", "--no-ff", "fugue/a-merged", "-m", "merge"],
    );
    run_git(&seed, &["push", "origin", "main"]);

    run_git(&seed, &["checkout", "-b", "fugue/a-unmerged"]);
    fs::write(seed.join("unmerged.txt"), "unmerged\n").unwrap();
    run_git(&seed, &["add", "."]);
    run_git(&seed, &["commit", "-m", "unmerged"]);
    run_git(&seed, &["push", "-u", "origin", "fugue/a-unmerged"]);

    run_git(&seed, &["checkout", "main"]);

    let mut dry = cargo_bin_cmd!("fugue");
    dry.current_dir(&seed);
    dry.args(["branch", "cleanup", "--dry-run"]);
    dry.assert()
        .success()
        .stdout(predicate::str::contains("Dry run - would delete:"))
        .stdout(predicate::str::contains("[remote]\tfugue/a-merged"))
        .stdout(predicate::str::contains("a-unmerged").not());

    let mut clean = cargo_bin_cmd!("fugue");
    clean.current_dir(&seed);
    clean.args(["branch", "cleanup"]);
    clean.assert().success().stdout("ok\n");

    let merged = Command::new("git")
        .current_dir(&seed)
        .args(["ls-remote", "--heads", "origin", "fugue/a-merged"])
        .output()
        .unwrap();
    assert!(
        merged.stdout.is_empty(),
        "merged branch should be deleted, got: {}",
        String::from_utf8_lossy(&merged.stdout)
    );

    let unmerged = Command::new("git")
        .current_dir(&seed)
        .args(["ls-remote", "--heads", "origin", "fugue/a-unmerged"])
        .output()
        .unwrap();
    assert!(!unmerged.stdout.is_empty(), "unmerged branch should remain");
}

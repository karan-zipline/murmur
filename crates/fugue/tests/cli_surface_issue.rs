use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn issue_help_hides_get_and_exposes_show() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["issue", "--help"]);

    let has_cmd = |name: &str| predicate::str::is_match(format!(r"(?m)^\s{{2}}{name}\b")).unwrap();

    cmd.assert()
        .success()
        .stdout(has_cmd("list"))
        .stdout(has_cmd("show"))
        .stdout(has_cmd("create"))
        .stdout(has_cmd("update"))
        .stdout(has_cmd("close"))
        .stdout(has_cmd("comment"))
        .stdout(has_cmd("plan"))
        .stdout(has_cmd("commit"))
        .stdout(has_cmd("get").not());
}

#[test]
fn issue_create_help_matches_fab_flags() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["issue", "create", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--type"))
        .stdout(predicate::str::contains("--depends-on"))
        .stdout(predicate::str::contains("--commit"))
        .stdout(predicate::str::contains("--parent"))
        .stdout(predicate::str::contains("--priority"))
        .stdout(predicate::str::contains("-d, --description"))
        .stdout(predicate::str::contains("--issue-type").not());
}

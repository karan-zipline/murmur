use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn help_includes_top_level_commands() {
    let mut cmd = cargo_bin_cmd!("mm");
    cmd.arg("--help");

    let has_cmd = |name: &str| predicate::str::is_match(format!(r"(?m)^\s{{2}}{name}\b")).unwrap();

    cmd.assert()
        .success()
        .stdout(has_cmd("agent"))
        .stdout(has_cmd("attach"))
        .stdout(has_cmd("branch"))
        .stdout(has_cmd("claims"))
        .stdout(has_cmd("commit"))
        .stdout(has_cmd("completion"))
        .stdout(has_cmd("director"))
        .stdout(has_cmd("issue"))
        .stdout(has_cmd("manager"))
        .stdout(has_cmd("plan"))
        .stdout(has_cmd("project"))
        .stdout(has_cmd("server"))
        .stdout(has_cmd("stats"))
        .stdout(has_cmd("status"))
        .stdout(has_cmd("tui"))
        .stdout(has_cmd("version"))
        // Hidden commands should not appear
        .stdout(has_cmd("hook").not())
        .stdout(has_cmd("permission").not())
        .stdout(has_cmd("ping").not())
        .stdout(has_cmd("question").not());
}

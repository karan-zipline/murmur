use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn has_cmd(name: &str) -> predicates::str::RegexPredicate {
    predicate::str::is_match(format!(r"(?m)^\s{{2}}{name}\b")).unwrap()
}

#[test]
fn agent_help_hides_internal_subcommands() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["agent", "--help"]);

    cmd.assert()
        .success()
        .stdout(has_cmd("list"))
        .stdout(has_cmd("abort"))
        .stdout(has_cmd("plan"))
        .stdout(has_cmd("claim"))
        .stdout(has_cmd("describe"))
        .stdout(has_cmd("done"))
        .stdout(has_cmd("create").not())
        .stdout(has_cmd("delete").not())
        .stdout(has_cmd("send-message").not())
        .stdout(has_cmd("tail").not())
        .stdout(has_cmd("chat-history").not());
}

#[test]
fn plan_help_is_storage_only() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["plan", "--help"]);

    cmd.assert()
        .success()
        .stdout(has_cmd("list"))
        .stdout(has_cmd("read"))
        .stdout(has_cmd("write"))
        .stdout(has_cmd("start").not())
        .stdout(has_cmd("stop").not())
        .stdout(has_cmd("list-running").not())
        .stdout(has_cmd("send-message").not())
        .stdout(has_cmd("chat-history").not());
}

#[test]
fn manager_help_hides_chat_commands() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["manager", "--help"]);

    cmd.assert()
        .success()
        .stdout(has_cmd("start"))
        .stdout(has_cmd("stop"))
        .stdout(has_cmd("status"))
        .stdout(has_cmd("clear"))
        .stdout(has_cmd("send-message").not())
        .stdout(has_cmd("chat-history").not());
}

#[test]
fn agent_done_help_includes_task_and_error_flags() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["agent", "done", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--task"))
        .stdout(predicate::str::contains("--error"))
        .stdout(predicate::str::contains("Arguments:").not());
}

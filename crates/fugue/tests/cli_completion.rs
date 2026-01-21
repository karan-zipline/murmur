use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn completion_zsh_prints_script() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["completion", "zsh"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("compdef").and(predicate::str::contains("fugue")));
}

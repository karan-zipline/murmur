use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn server_start_supports_short_foreground_flag() {
    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.args(["server", "start", "-f", "--help"]);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("-f").and(predicate::str::contains("--foreground")));
}

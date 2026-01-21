use std::fs;
use std::os::unix::fs::PermissionsExt as _;

use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

#[test]
fn logging_does_not_panic_when_fugue_dir_not_writable() {
    let dir = TempDir::new().unwrap();
    let fugue_dir = dir.path().join("fugue-ro");
    fs::create_dir_all(&fugue_dir).unwrap();
    let mut perms = fs::metadata(&fugue_dir).unwrap().permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&fugue_dir, perms).unwrap();

    let mut cmd = cargo_bin_cmd!("fugue");
    cmd.env("FUGUE_DIR", &fugue_dir);
    cmd.args(["version"]);
    cmd.assert().success();
}

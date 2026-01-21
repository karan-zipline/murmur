use std::ffi::OsStr;
use std::path::Path;

pub(in crate::daemon) const HOOK_EXE_ENV: &str = "FUGUE_HOOK_EXE";

pub(in crate::daemon) fn hook_exe_prefix_from(
    env_override: Option<&OsStr>,
    current_exe: Option<&Path>,
) -> String {
    if let Some(prefix) = env_override
        .and_then(|v| v.to_str())
        .map(sanitize_hook_exe_prefix)
        .filter(|s| !s.is_empty())
    {
        return prefix;
    }

    if let Some(prefix) = current_exe
        .and_then(|p| p.to_str())
        .map(sanitize_hook_exe_prefix)
        .filter(|s| !s.is_empty())
    {
        return prefix;
    }

    "murmur".to_owned()
}

pub(in crate::daemon) fn hook_exe_prefix() -> String {
    let env_override = std::env::var_os(HOOK_EXE_ENV);
    let current_exe = std::env::current_exe().ok();
    hook_exe_prefix_from(env_override.as_deref(), current_exe.as_deref())
}

fn sanitize_hook_exe_prefix(input: &str) -> String {
    let trimmed = input.trim();
    let trimmed = trimmed.strip_suffix(" (deleted)").unwrap_or(trimmed);
    trimmed.trim().to_owned()
}

fn shell_escape_posix(word: &str) -> String {
    let mut escaped = String::with_capacity(word.len() + 2);
    escaped.push('\'');
    for ch in word.chars() {
        if ch == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

pub(in crate::daemon) fn render_shell_command(words: &[&str]) -> String {
    words
        .iter()
        .copied()
        .map(shell_escape_posix)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;
    use std::process::Command;

    #[test]
    fn hook_exe_prefix_strips_deleted_suffix() {
        let current = Path::new("/tmp/murmur (deleted)");
        assert_eq!(
            hook_exe_prefix_from(None, Some(current)),
            "/tmp/murmur".to_owned()
        );
    }

    #[test]
    fn hook_exe_prefix_env_override_wins() {
        let current = Path::new("/tmp/murmur");
        let override_exe = OsStr::new("/opt/murmur");
        assert_eq!(
            hook_exe_prefix_from(Some(override_exe), Some(current)),
            "/opt/murmur".to_owned()
        );
    }

    #[test]
    fn hook_exe_prefix_env_override_is_sanitized() {
        let current = Path::new("/tmp/murmur");
        let override_exe = OsStr::new(" /opt/murmur (deleted) ");
        assert_eq!(
            hook_exe_prefix_from(Some(override_exe), Some(current)),
            "/opt/murmur".to_owned()
        );
    }

    #[test]
    fn hook_exe_prefix_falls_back_on_non_utf8_override_and_exe() {
        let override_exe = OsString::from_vec(vec![0xff, 0xfe]);
        let current = PathBuf::from(OsString::from_vec(vec![0xff, 0xfe, 0xfd]));
        assert_eq!(
            hook_exe_prefix_from(Some(&override_exe), Some(&current)),
            "murmur".to_owned()
        );
    }

    #[test]
    fn hook_exe_prefix_never_empty() {
        let override_exe = OsStr::new("   ");
        assert_eq!(
            hook_exe_prefix_from(Some(override_exe), None),
            "murmur".to_owned()
        );
    }

    #[test]
    fn render_shell_command_runs_via_sh_c_with_spacey_exe_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let bin_dir = dir.path().join("bin with spaces");
        fs::create_dir_all(&bin_dir).unwrap();

        let exe_path = bin_dir.join("murmur-test");
        fs::write(
            &exe_path,
            r#"#!/usr/bin/env sh
set -eu
printf "%s" "ok"
"#,
        )
        .unwrap();

        let mut perms = fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).unwrap();

        let cmd = render_shell_command(&[exe_path.to_str().unwrap()]);
        let out = Command::new("sh").arg("-c").arg(cmd).output().unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "ok");
    }
}

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathInputs {
    pub home_dir: PathBuf,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub murmur_dir_override: Option<PathBuf>,
    pub socket_path_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MurmurPaths {
    pub murmur_dir: PathBuf,

    pub socket_path: PathBuf,
    pub pid_path: PathBuf,
    pub log_path: PathBuf,

    pub plans_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub projects_dir: PathBuf,

    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub permissions_file: PathBuf,
}

pub fn compute_paths(inputs: PathInputs) -> MurmurPaths {
    let murmur_dir = inputs
        .murmur_dir_override
        .clone()
        .unwrap_or_else(|| inputs.home_dir.join(".murmur"));

    let config_base = match inputs.murmur_dir_override {
        Some(ref override_dir) => override_dir.join("config"),
        None => inputs
            .xdg_config_home
            .unwrap_or_else(|| inputs.home_dir.join(".config"))
            .join("murmur"),
    };

    // Match fab's behavior: keep the socket under the base directory by default.
    // This avoids surprises when XDG_RUNTIME_DIR points somewhere ephemeral.
    let socket_path = if let Some(socket_path_override) = inputs.socket_path_override {
        socket_path_override
    } else {
        murmur_dir.join("murmur.sock")
    };

    MurmurPaths {
        socket_path,
        pid_path: murmur_dir.join("murmur.pid"),
        log_path: murmur_dir.join("murmur.log"),
        plans_dir: murmur_dir.join("plans"),
        runtime_dir: murmur_dir.join("runtime"),
        projects_dir: murmur_dir.join("projects"),

        config_file: config_base.join("config.toml"),
        permissions_file: config_base.join("permissions.toml"),

        murmur_dir,
        config_dir: config_base,
    }
}

#[derive(Debug, Error)]
pub enum SafeJoinError {
    #[error("path segment is empty")]
    Empty,
    #[error("path segment is not a normal component: {segment:?}")]
    NotNormal { segment: String },
}

pub fn safe_join(base: &Path, segment: &str) -> Result<PathBuf, SafeJoinError> {
    let segment = segment.trim();
    if segment.is_empty() {
        return Err(SafeJoinError::Empty);
    }

    let segment_path = Path::new(segment);
    if segment_path.is_absolute() {
        return Err(SafeJoinError::NotNormal {
            segment: segment.to_owned(),
        });
    }

    let mut components = segment_path.components();
    let first = components.next();
    let second = components.next();
    match (first, second) {
        (Some(std::path::Component::Normal(_)), None) => Ok(base.join(segment)),
        _ => Err(SafeJoinError::NotNormal {
            segment: segment.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_paths_default() {
        let inputs = PathInputs {
            home_dir: PathBuf::from("/home/alice"),
            xdg_config_home: None,
            xdg_runtime_dir: None,
            murmur_dir_override: None,
            socket_path_override: None,
        };

        let got = compute_paths(inputs);
        assert_eq!(got.murmur_dir, PathBuf::from("/home/alice/.murmur"));
        assert_eq!(
            got.log_path,
            PathBuf::from("/home/alice/.murmur/murmur.log")
        );
        assert_eq!(got.config_dir, PathBuf::from("/home/alice/.config/murmur"));
        assert_eq!(
            got.socket_path,
            PathBuf::from("/home/alice/.murmur/murmur.sock")
        );
        assert_eq!(
            got.config_file,
            PathBuf::from("/home/alice/.config/murmur/config.toml")
        );
    }

    #[test]
    fn compute_paths_uses_xdg_config_home() {
        let inputs = PathInputs {
            home_dir: PathBuf::from("/home/alice"),
            xdg_config_home: Some(PathBuf::from("/tmp/xdg")),
            xdg_runtime_dir: None,
            murmur_dir_override: None,
            socket_path_override: None,
        };

        let got = compute_paths(inputs);
        assert_eq!(got.config_dir, PathBuf::from("/tmp/xdg/murmur"));
    }

    #[test]
    fn compute_paths_ignores_xdg_runtime_dir_for_socket_by_default() {
        let inputs = PathInputs {
            home_dir: PathBuf::from("/home/alice"),
            xdg_config_home: None,
            xdg_runtime_dir: Some(PathBuf::from("/run/user/123")),
            murmur_dir_override: None,
            socket_path_override: None,
        };

        let got = compute_paths(inputs);
        assert_eq!(
            got.socket_path,
            PathBuf::from("/home/alice/.murmur/murmur.sock")
        );
        assert_eq!(got.murmur_dir, PathBuf::from("/home/alice/.murmur"));
    }

    #[test]
    fn compute_paths_murmur_dir_override_overrides_config() {
        let inputs = PathInputs {
            home_dir: PathBuf::from("/home/alice"),
            xdg_config_home: Some(PathBuf::from("/tmp/xdg")),
            xdg_runtime_dir: Some(PathBuf::from("/run/user/123")),
            murmur_dir_override: Some(PathBuf::from("/tmp/murmur-dev")),
            socket_path_override: None,
        };

        let got = compute_paths(inputs);
        assert_eq!(got.murmur_dir, PathBuf::from("/tmp/murmur-dev"));
        assert_eq!(got.config_dir, PathBuf::from("/tmp/murmur-dev/config"));
        assert_eq!(
            got.socket_path,
            PathBuf::from("/tmp/murmur-dev/murmur.sock")
        );
    }

    #[test]
    fn safe_join_allows_single_normal_segment() {
        let base = Path::new("/base");
        let got = safe_join(base, "wt-123").unwrap();
        assert_eq!(got, PathBuf::from("/base/wt-123"));
    }

    #[test]
    fn safe_join_rejects_empty() {
        let base = Path::new("/base");
        assert!(matches!(safe_join(base, ""), Err(SafeJoinError::Empty)));
    }

    #[test]
    fn safe_join_rejects_path_traversal() {
        let base = Path::new("/base");
        assert!(matches!(
            safe_join(base, "../evil"),
            Err(SafeJoinError::NotNormal { .. })
        ));
    }

    #[test]
    fn safe_join_rejects_nested_paths() {
        let base = Path::new("/base");
        assert!(matches!(
            safe_join(base, "a/b"),
            Err(SafeJoinError::NotNormal { .. })
        ));
    }

    #[test]
    fn safe_join_rejects_absolute_paths() {
        let base = Path::new("/base");
        assert!(matches!(
            safe_join(base, "/abs"),
            Err(SafeJoinError::NotNormal { .. })
        ));
    }
}

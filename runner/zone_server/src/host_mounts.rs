//! Host folders bind-mounted into the manager for chat tools.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Where compose bind-mounts `ZONE_HOST_ROOT` inside the manager.
pub const CONTAINER_ROOT: &str = "/host";

/// Instance mapping from host paths the UI stores to paths the process can open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostMounts {
    pub host_root: Option<PathBuf>,
    pub in_container: bool,
}

/// Why a host path cannot be used as a folder on this instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    Unset,
    Invalid,
    Outside,
    Missing,
}

impl MapError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unset => "host_root_unset",
            Self::Invalid => "invalid_path",
            Self::Outside => "outside_host_root",
            Self::Missing => "not_a_directory",
        }
    }

    pub fn message(&self, host_root: Option<&Path>, host: &str) -> String {
        match self {
            Self::Unset => {
                "Set ZONE_HOST_ROOT to a host folder and recreate the manager so chats can see it."
                    .to_string()
            }
            Self::Invalid => "Each folder must be an absolute path without ..".to_string(),
            Self::Outside => match host_root {
                Some(root) => format!(
                    "Folder must be under ZONE_HOST_ROOT ({}). Set ZONE_HOST_ROOT to a parent of this folder and recreate the manager.",
                    root.display()
                ),
                None => {
                    "Folder is outside ZONE_HOST_ROOT. Set ZONE_HOST_ROOT and recreate the manager."
                        .to_string()
                }
            },
            Self::Missing => format!("Not a directory: {host}"),
        }
    }
}

/// Collapse `.`, refuse `..`, and require an absolute path.
pub fn normalize(raw: &str) -> Result<PathBuf, MapError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(MapError::Invalid);
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(MapError::Invalid);
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => out.push(component),
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            Component::Prefix(_) | Component::ParentDir => return Err(MapError::Invalid),
        }
    }
    if !out.is_absolute() {
        return Err(MapError::Invalid);
    }
    Ok(out)
}

impl HostMounts {
    pub fn from_env() -> Self {
        let host_root = std::env::var("ZONE_HOST_ROOT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .and_then(|value| match normalize(&value) {
                Ok(path) => Some(path),
                Err(_) => {
                    tracing::warn!(
                        path = value,
                        "ZONE_HOST_ROOT must be an absolute path without ..; ignoring it"
                    );
                    None
                }
            });
        let in_container = matches!(
            std::env::var("ZONE_IN_CONTAINER")
                .ok()
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("1" | "true" | "yes" | "on")
        );
        Self {
            host_root,
            in_container,
        }
    }

    pub fn container_root(&self) -> Option<&'static str> {
        self.in_container.then_some(CONTAINER_ROOT)
    }

    pub fn ready(&self) -> bool {
        !self.in_container || self.host_root.is_some()
    }

    pub fn hint(&self) -> String {
        if !self.in_container {
            return "Paths are used as they are on this machine.".to_string();
        }
        match &self.host_root {
            Some(root) => format!(
                "Folders must live under {}. Recreate the manager after changing ZONE_HOST_ROOT.",
                root.display()
            ),
            None => "Set ZONE_HOST_ROOT to a host folder (often your home) and recreate the manager so chats can see it.".to_string(),
        }
    }

    /// Host path the UI stores → path this process opens.
    pub fn map(&self, host: &str) -> Result<PathBuf, MapError> {
        let host = normalize(host)?;
        if !self.in_container {
            return Ok(host);
        }
        let Some(root) = self.host_root.as_ref() else {
            return Err(MapError::Unset);
        };
        if host == *root {
            return Ok(PathBuf::from(CONTAINER_ROOT));
        }
        let relative = host.strip_prefix(root).map_err(|_| MapError::Outside)?;
        if relative.as_os_str().is_empty() {
            return Ok(PathBuf::from(CONTAINER_ROOT));
        }
        Ok(PathBuf::from(CONTAINER_ROOT).join(relative))
    }

    pub fn mapped_directory(&self, host: &str) -> Result<PathBuf, MapError> {
        let mapped = self.map(host)?;
        if mapped.is_dir() {
            Ok(mapped)
        } else {
            Err(MapError::Missing)
        }
    }

    /// First mapped folder that exists here, otherwise `fallback`.
    pub fn chat_cwd(&self, directories: &[String], fallback: PathBuf) -> PathBuf {
        for host in directories {
            if let Ok(mapped) = self.map(host) {
                if mapped.is_dir() {
                    return mapped;
                }
            }
        }
        fallback
    }

    /// Normalize, map, and require each path to exist as a directory.
    pub fn validate(&self, directories: &[String]) -> Result<Vec<String>, (MapError, String)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for raw in directories {
            let host = normalize(raw).map_err(|error| (error, raw.clone()))?;
            let key = host.to_string_lossy().into_owned();
            if !seen.insert(key.clone()) {
                continue;
            }
            self.mapped_directory(&key)
                .map_err(|error| (error, key.clone()))?;
            out.push(key);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn container(root: &str) -> HostMounts {
        HostMounts {
            host_root: Some(PathBuf::from(root)),
            in_container: true,
        }
    }

    #[test]
    fn native_mapping_is_the_host_path() {
        let mounts = HostMounts::default();
        assert_eq!(
            mounts.map("/Users/jake/Local/jbs").unwrap(),
            PathBuf::from("/Users/jake/Local/jbs")
        );
    }

    #[test]
    fn container_mapping_strips_the_host_root() {
        let mounts = container("/Users/jake/Local");
        assert_eq!(
            mounts.map("/Users/jake/Local/jbs").unwrap(),
            PathBuf::from("/host/jbs")
        );
        assert_eq!(
            mounts.map("/Users/jake/Local").unwrap(),
            PathBuf::from("/host")
        );
        assert_eq!(
            mounts.map("/Users/jake/Local/").unwrap(),
            PathBuf::from("/host")
        );
    }

    #[test]
    fn a_path_outside_the_host_root_is_refused() {
        let mounts = container("/Users/jake/Local");
        assert_eq!(mounts.map("/Users/jake/Other"), Err(MapError::Outside));
        assert_eq!(mounts.map("/Users/jake/Localish"), Err(MapError::Outside));
    }

    #[test]
    fn an_unset_host_root_in_a_container_cannot_map() {
        let mounts = HostMounts {
            host_root: None,
            in_container: true,
        };
        assert_eq!(mounts.map("/Users/jake/Local/jbs"), Err(MapError::Unset));
        assert!(!mounts.ready());
    }

    #[test]
    fn relative_and_parent_paths_are_invalid() {
        assert_eq!(normalize("Local/jbs"), Err(MapError::Invalid));
        assert_eq!(normalize("/Users/jake/../etc"), Err(MapError::Invalid));
        assert_eq!(normalize(""), Err(MapError::Invalid));
        assert_eq!(
            normalize("/Users/jake/./Local/jbs").unwrap(),
            PathBuf::from("/Users/jake/Local/jbs")
        );
    }

    #[test]
    fn validate_keeps_order_and_drops_duplicates() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("one");
        let second = dir.path().join("two");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let mounts = HostMounts::default();
        let first_s = first.to_string_lossy().into_owned();
        let second_s = second.to_string_lossy().into_owned();
        let saved = mounts
            .validate(&[format!("{first_s}/"), second_s.clone(), first_s.clone()])
            .unwrap();
        assert_eq!(saved, vec![first_s, second_s]);
    }

    #[test]
    fn validate_requires_a_directory_that_exists() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("gone");
        let mounts = HostMounts::default();
        let error = mounts
            .validate(&[missing.to_string_lossy().into_owned()])
            .unwrap_err();
        assert_eq!(error.0, MapError::Missing);
    }

    #[test]
    fn chat_cwd_picks_the_first_directory_that_exists() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("gone");
        let present = dir.path().join("here");
        fs::create_dir(&present).unwrap();
        let mounts = HostMounts::default();
        let cwd = mounts.chat_cwd(
            &[
                missing.to_string_lossy().into_owned(),
                present.to_string_lossy().into_owned(),
            ],
            PathBuf::from("/fallback"),
        );
        assert_eq!(cwd, present);
    }

    #[test]
    fn chat_cwd_falls_back_when_nothing_maps() {
        let mounts = HostMounts {
            host_root: None,
            in_container: true,
        };
        assert_eq!(
            mounts.chat_cwd(
                &["/Users/jake/Local/jbs".into()],
                PathBuf::from("/fallback")
            ),
            PathBuf::from("/fallback")
        );
    }
}

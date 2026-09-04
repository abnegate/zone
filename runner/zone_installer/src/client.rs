//! Platform-specific paths for the Zone desktop, Android, and iOS clients.

use std::path::PathBuf;

use crate::frontend::{self, FrontendKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPlatform {
    Desktop,
    Android,
    Ios,
}

impl ClientPlatform {
    pub fn current() -> Self {
        Self::from_os(std::env::consts::OS)
    }

    pub fn from_os(os: &str) -> Self {
        match os {
            "android" => Self::Android,
            "ios" => Self::Ios,
            _ => Self::Desktop,
        }
    }

    pub fn is_mobile(self) -> bool {
        matches!(self, Self::Android | Self::Ios)
    }

    pub fn uses_system_share_dir(self) -> bool {
        !self.is_mobile()
    }
}

/// Resolve the client config file for a platform.
///
/// Desktop always uses `desktop_fallback` (`~/.zone/config.toml` or
/// `ZONE_CONFIG_PATH`). Android and iOS write into the app config directory
/// when Tauri provides one.
pub fn config_path(
    platform: ClientPlatform,
    app_config_dir: Option<PathBuf>,
    desktop_fallback: PathBuf,
) -> PathBuf {
    if platform.is_mobile()
        && let Some(dir) = app_config_dir
    {
        return dir.join("config.toml");
    }
    desktop_fallback
}

#[derive(Debug, Clone)]
pub struct ManagerDirInputs {
    pub platform: ClientPlatform,
    pub resource_dir: Option<PathBuf>,
    pub app_data_dir: Option<PathBuf>,
    pub cwd: PathBuf,
    pub system_share_dir: Option<PathBuf>,
}

/// Locate the bundled manager SPA for the Tauri client.
pub fn resolve_manager_dir(inputs: ManagerDirInputs) -> PathBuf {
    if let Some(dir) = inputs.resource_dir {
        for candidate in [dir.join("manager"), dir.clone()] {
            if candidate.join("index.html").exists() {
                return candidate;
            }
        }
    }

    if let Some(dir) = inputs.app_data_dir {
        let manager = dir.join("manager");
        if manager.join("index.html").exists() {
            return manager;
        }
    }

    if inputs.platform.uses_system_share_dir()
        && let Some(share) = inputs.system_share_dir
        && share.join("index.html").exists()
    {
        return share;
    }

    for root in [
        inputs.cwd.clone(),
        inputs.cwd.join(".."),
        inputs.cwd.join("../.."),
    ] {
        let manager = root.join("manager/frontend/build");
        if manager.join("index.html").exists() {
            return manager;
        }
    }

    frontend::resolve_frontend_dir(FrontendKind::Manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write_index(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("index.html"), "<html></html>").unwrap();
    }

    fn empty_inputs(platform: ClientPlatform, cwd: PathBuf) -> ManagerDirInputs {
        ManagerDirInputs {
            platform,
            resource_dir: None,
            app_data_dir: None,
            cwd,
            system_share_dir: None,
        }
    }

    #[test]
    fn maps_os_names_to_platforms() {
        assert_eq!(ClientPlatform::from_os("android"), ClientPlatform::Android);
        assert_eq!(ClientPlatform::from_os("ios"), ClientPlatform::Ios);
        assert_eq!(ClientPlatform::from_os("macos"), ClientPlatform::Desktop);
        assert_eq!(ClientPlatform::from_os("linux"), ClientPlatform::Desktop);
        assert_eq!(ClientPlatform::from_os("windows"), ClientPlatform::Desktop);
        assert!(!ClientPlatform::Android.uses_system_share_dir());
        assert!(!ClientPlatform::Ios.uses_system_share_dir());
        assert!(ClientPlatform::Desktop.uses_system_share_dir());
        assert!(ClientPlatform::Android.is_mobile());
        assert!(ClientPlatform::Ios.is_mobile());
        assert!(!ClientPlatform::Desktop.is_mobile());
    }

    #[test]
    fn current_platform_matches_host_os() {
        assert_eq!(
            ClientPlatform::current(),
            ClientPlatform::from_os(std::env::consts::OS)
        );
    }

    #[test]
    fn desktop_config_ignores_app_config_dir() {
        let path = config_path(
            ClientPlatform::Desktop,
            Some(PathBuf::from("/data/user/0/com.abnegate.zone/files")),
            PathBuf::from("/home/zone/.zone/config.toml"),
        );
        assert_eq!(path, PathBuf::from("/home/zone/.zone/config.toml"));
    }

    #[test]
    fn android_config_uses_app_config_dir() {
        let path = config_path(
            ClientPlatform::Android,
            Some(PathBuf::from("/data/user/0/com.abnegate.zone/files")),
            PathBuf::from("/home/zone/.zone/config.toml"),
        );
        assert_eq!(
            path,
            PathBuf::from("/data/user/0/com.abnegate.zone/files/config.toml")
        );
    }

    #[test]
    fn ios_config_uses_app_config_dir() {
        let path = config_path(
            ClientPlatform::Ios,
            Some(PathBuf::from(
                "/var/mobile/Containers/Data/Application/Zone/Library/Application Support",
            )),
            PathBuf::from("/Users/zone/.zone/config.toml"),
        );
        assert_eq!(
            path,
            PathBuf::from(
                "/var/mobile/Containers/Data/Application/Zone/Library/Application Support/config.toml"
            )
        );
    }

    #[test]
    fn mobile_config_falls_back_without_app_dir() {
        let fallback = PathBuf::from("/tmp/zone-fallback.toml");
        assert_eq!(
            config_path(ClientPlatform::Android, None, fallback.clone()),
            fallback
        );
        assert_eq!(
            config_path(ClientPlatform::Ios, None, fallback.clone()),
            fallback
        );
    }

    #[test]
    fn prefers_bundled_resource_manager() {
        let root = tempfile::tempdir().unwrap();
        let resource = root.path().join("resources");
        write_index(&resource.join("manager"));
        write_index(&root.path().join("data/manager"));
        write_index(&root.path().join("share"));

        for platform in [
            ClientPlatform::Desktop,
            ClientPlatform::Android,
            ClientPlatform::Ios,
        ] {
            let resolved = resolve_manager_dir(ManagerDirInputs {
                platform,
                resource_dir: Some(resource.clone()),
                app_data_dir: Some(root.path().join("data")),
                cwd: root.path().join("cwd"),
                system_share_dir: Some(root.path().join("share")),
            });
            assert_eq!(resolved, resource.join("manager"));
        }
    }

    #[test]
    fn accepts_resource_dir_that_is_the_spa() {
        let root = tempfile::tempdir().unwrap();
        let resource = root.path().join("resources");
        write_index(&resource);
        let resolved = resolve_manager_dir(ManagerDirInputs {
            platform: ClientPlatform::Android,
            resource_dir: Some(resource.clone()),
            app_data_dir: None,
            cwd: root.path().to_path_buf(),
            system_share_dir: None,
        });
        assert_eq!(resolved, resource);
    }

    #[test]
    fn uses_app_data_manager_when_resources_missing() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("app-data/manager");
        write_index(&data);
        for platform in [ClientPlatform::Android, ClientPlatform::Ios] {
            let resolved = resolve_manager_dir(ManagerDirInputs {
                platform,
                resource_dir: Some(root.path().join("missing")),
                app_data_dir: Some(root.path().join("app-data")),
                cwd: root.path().to_path_buf(),
                system_share_dir: None,
            });
            assert_eq!(resolved, data);
        }
    }

    #[test]
    fn desktop_uses_system_share_before_cwd() {
        let root = tempfile::tempdir().unwrap();
        let share = root.path().join("usr/share/zone/manager");
        write_index(&share);
        write_index(&root.path().join("cwd/manager/frontend/build"));

        let desktop = resolve_manager_dir(ManagerDirInputs {
            platform: ClientPlatform::Desktop,
            resource_dir: None,
            app_data_dir: None,
            cwd: root.path().join("cwd"),
            system_share_dir: Some(share.clone()),
        });
        assert_eq!(desktop, share);
    }

    #[test]
    fn mobile_skips_system_share_dir() {
        let root = tempfile::tempdir().unwrap();
        let share = root.path().join("usr/share/zone/manager");
        write_index(&share);
        let cwd_manager = root.path().join("cwd/manager/frontend/build");
        write_index(&cwd_manager);

        for platform in [ClientPlatform::Android, ClientPlatform::Ios] {
            let resolved = resolve_manager_dir(ManagerDirInputs {
                platform,
                resource_dir: None,
                app_data_dir: None,
                cwd: root.path().join("cwd"),
                system_share_dir: Some(share.clone()),
            });
            assert_eq!(resolved, cwd_manager);
        }
    }

    #[test]
    fn walks_parent_cwds_for_dev_checkout() {
        let root = tempfile::tempdir().unwrap();
        let build = root.path().join("manager/frontend/build");
        write_index(&build);
        let nested = root.path().join("runner/zone_desktop");
        fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_manager_dir(empty_inputs(ClientPlatform::Desktop, nested));
        assert_eq!(
            resolved.canonicalize().unwrap(),
            build.canonicalize().unwrap()
        );
    }
}

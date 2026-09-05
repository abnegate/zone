//! Resolve bundled installer and manager frontend directories.

use std::path::{Component, Path, PathBuf};

pub const FRONTEND_DIR_ENV: &str = "ZONE_FRONTEND_DIR";
pub const BIND_ENV: &str = "ZONE_BIND";
pub const MODE_ENV: &str = "ZONE_MODE";
pub const PROXY_TARGET_ENV: &str = "ZONE_PROXY_TARGET";
pub const CONFIG_PATH_ENV: &str = "ZONE_CONFIG_PATH";

const DEFAULT_BIND: &str = "0.0.0.0:8000";
const DEFAULT_INSTALLER_DIR: &str = "frontend/build";
const DOCKER_INSTALLER_DIR: &str = "/app/frontend/build";
const DEBIAN_INSTALLER_DIR: &str = "/usr/share/zone/installer";
const DEBIAN_MANAGER_DIR: &str = "/usr/share/zone/manager";
const DEFAULT_PROXY_TARGET: &str = "http://manager.localhost";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendKind {
    Installer,
    Manager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Install,
    Console,
    Setup,
}

pub fn app_mode() -> AppMode {
    match std::env::var(MODE_ENV)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "console" | "manager" | "ui" => AppMode::Console,
        _ => AppMode::Install,
    }
}

pub fn bind_addr() -> String {
    std::env::var(BIND_ENV).unwrap_or_else(|_| DEFAULT_BIND.to_string())
}

pub fn proxy_target() -> String {
    proxy_target_from(&config_file())
}

pub fn proxy_target_from(path: &Path) -> String {
    configured_host_from(path).unwrap_or_else(|| DEFAULT_PROXY_TARGET.to_string())
}

pub fn is_configured() -> bool {
    is_configured_at(&config_file())
}

pub fn is_configured_at(path: &Path) -> bool {
    env_proxy_target().is_some() || read_host_from_path(path).is_some()
}

pub fn config_file() -> PathBuf {
    config_file_from(
        std::env::var(CONFIG_PATH_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        dirs::home_dir(),
    )
}

pub fn config_file_from(env_path: Option<String>, home: Option<PathBuf>) -> PathBuf {
    if let Some(path) = env_path {
        return PathBuf::from(path);
    }
    home.unwrap_or_else(|| PathBuf::from("."))
        .join(".zone/config.toml")
}

fn env_proxy_target() -> Option<String> {
    let target = std::env::var(PROXY_TARGET_ENV).ok()?;
    let trimmed = target.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.trim_end_matches('/').to_string())
    }
}

pub fn configured_host() -> Option<String> {
    configured_host_from(&config_file())
}

pub fn configured_host_from(path: &Path) -> Option<String> {
    env_proxy_target().or_else(|| read_host_from_path(path))
}

pub fn normalize_host(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter a Zone server URL".into());
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("URL must start with http:// or https://".into());
    }
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    if without_scheme.is_empty()
        || without_scheme.contains(char::is_whitespace)
        || without_scheme.contains(['"', '\\'])
    {
        return Err("Enter a valid Zone server URL".into());
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

pub fn write_host(host: &str) -> std::io::Result<()> {
    write_host_to(&config_file(), host)
}

pub fn write_host_to(path: &Path, host: &str) -> std::io::Result<()> {
    let path = sanitize_writable_config_path(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let host_line = format!("host = \"{host}\"");
    let mut replaced = false;
    let mut out = String::new();
    if path.exists() {
        let existing = std::fs::read_to_string(&path)?;
        for line in existing.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("host")
                && rest.trim_start().starts_with('=')
            {
                out.push_str(&host_line);
                out.push('\n');
                replaced = true;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if !replaced {
        out.push_str(&host_line);
        out.push('\n');
    }
    std::fs::write(&path, out)
}

fn sanitize_writable_config_path(path: &Path) -> std::io::Result<PathBuf> {
    let raw = path.to_string_lossy();
    // CodeQL rust/path-injection: reject parent-directory segments before any write.
    if raw.contains("..") || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid config path",
        ));
    }
    Ok(PathBuf::from(raw.as_ref()))
}

pub fn resolve_frontend_dir(kind: FrontendKind) -> PathBuf {
    resolve_frontend_dir_from(
        kind,
        std::env::var_os(FRONTEND_DIR_ENV).map(PathBuf::from),
        std::env::current_exe().ok(),
    )
}

pub fn resolve_frontend_dir_from(
    kind: FrontendKind,
    env_dir: Option<PathBuf>,
    exe: Option<PathBuf>,
) -> PathBuf {
    if let Some(dir) = env_dir
        && !dir.as_os_str().is_empty()
    {
        return dir;
    }

    if kind == FrontendKind::Installer {
        let docker = Path::new(DOCKER_INSTALLER_DIR);
        if docker.join("index.html").exists() {
            return docker.to_path_buf();
        }
    }

    if let Some(exe) = exe
        && let Some(parent) = exe.parent()
    {
        for candidate in bundled_candidates(parent, kind) {
            if candidate.join("index.html").exists() {
                return candidate.canonicalize().unwrap_or(candidate);
            }
        }
    }

    let system = match kind {
        FrontendKind::Installer => PathBuf::from(DEBIAN_INSTALLER_DIR),
        FrontendKind::Manager => PathBuf::from(DEBIAN_MANAGER_DIR),
    };
    if system.join("index.html").exists() {
        return system;
    }

    match kind {
        FrontendKind::Installer => PathBuf::from(DEFAULT_INSTALLER_DIR),
        FrontendKind::Manager => PathBuf::from("manager/frontend/build"),
    }
}

fn bundled_candidates(exe_dir: &Path, kind: FrontendKind) -> Vec<PathBuf> {
    match kind {
        FrontendKind::Installer => vec![
            exe_dir.join("../Resources/installer"),
            exe_dir.join("frontend/build"),
        ],
        FrontendKind::Manager => vec![
            exe_dir.join("../Resources/manager"),
            exe_dir.join("manager/frontend/build"),
        ],
    }
}

fn read_host_from_path(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    host_from_toml(&content)
}

fn host_from_toml(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("host") {
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let rest = rest.trim();
            let value = rest.trim_matches('"').trim_matches('\'').trim();
            if !value.is_empty() {
                return Some(value.trim_end_matches('/').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_index(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("index.html"), "<html></html>").unwrap();
    }

    #[test]
    fn env_dir_wins() {
        let dir = PathBuf::from("/tmp/zone-frontend-override");
        let resolved = resolve_frontend_dir_from(FrontendKind::Installer, Some(dir.clone()), None);
        assert_eq!(resolved, dir);
    }

    #[test]
    fn finds_app_bundle_installer() {
        let root = tempfile::tempdir().unwrap();
        let macos = root.path().join("Zone.app/Contents/MacOS");
        let installer = root.path().join("Zone.app/Contents/Resources/installer");
        fs::create_dir_all(&macos).unwrap();
        write_index(&installer);
        let exe = macos.join("zone-installer");
        fs::write(&exe, []).unwrap();

        let resolved = resolve_frontend_dir_from(FrontendKind::Installer, None, Some(exe));
        assert_eq!(resolved, installer.canonicalize().unwrap());
    }

    #[test]
    fn finds_app_bundle_manager() {
        let root = tempfile::tempdir().unwrap();
        let macos = root.path().join("Zone.app/Contents/MacOS");
        let manager = root.path().join("Zone.app/Contents/Resources/manager");
        fs::create_dir_all(&macos).unwrap();
        write_index(&manager);
        let exe = macos.join("zone-installer");
        fs::write(&exe, []).unwrap();

        let resolved = resolve_frontend_dir_from(FrontendKind::Manager, None, Some(exe));
        assert_eq!(resolved, manager.canonicalize().unwrap());
    }

    #[test]
    fn defaults_when_missing() {
        let resolved = resolve_frontend_dir_from(
            FrontendKind::Installer,
            None,
            Some(PathBuf::from("/tmp/does-not-exist/zone-installer")),
        );
        assert_eq!(resolved, PathBuf::from(DEFAULT_INSTALLER_DIR));
    }

    #[test]
    fn parse_host_from_toml() {
        let content = "model = \"gpt-4o\"\nhost = \"https://zone.example.com/\"\n";
        assert_eq!(
            host_from_toml(content).as_deref(),
            Some("https://zone.example.com")
        );
    }

    #[test]
    fn parse_host_skips_near_miss_keys() {
        let content = "hosts = [\"a\"]\nhost = \"https://zone.example.com\"\n";
        assert_eq!(
            host_from_toml(content).as_deref(),
            Some("https://zone.example.com")
        );
    }

    #[test]
    fn normalize_host_strips_slash() {
        assert_eq!(
            normalize_host("https://zone.example.com/").unwrap(),
            "https://zone.example.com"
        );
    }

    #[test]
    fn normalize_host_rejects_scheme() {
        assert!(normalize_host("zone.example.com").is_err());
    }

    #[test]
    fn normalize_host_rejects_quotes() {
        assert!(normalize_host(r#"https://a"b.example"#).is_err());
        assert!(normalize_host(r"https://a\b.example").is_err());
    }

    #[test]
    fn config_file_prefers_env_path() {
        assert_eq!(
            config_file_from(
                Some("/tmp/zone-client.toml".into()),
                Some(PathBuf::from("/home/zone"))
            ),
            PathBuf::from("/tmp/zone-client.toml")
        );
    }

    #[test]
    fn config_file_defaults_to_home_zone() {
        assert_eq!(
            config_file_from(None, Some(PathBuf::from("/home/zone"))),
            PathBuf::from("/home/zone/.zone/config.toml")
        );
    }

    #[test]
    fn proxy_target_from_reads_saved_host() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        assert_eq!(proxy_target_from(&path), DEFAULT_PROXY_TARGET);
        fs::write(&path, "host = \"https://zone.example.com\"\n").unwrap();
        assert_eq!(proxy_target_from(&path), "https://zone.example.com");
    }

    #[test]
    fn reads_host_from_config_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        assert!(read_host_from_path(&path).is_none());
        fs::write(&path, "host = \"https://zone.example.com\"\n").unwrap();
        assert_eq!(
            read_host_from_path(&path).as_deref(),
            Some("https://zone.example.com")
        );
    }

    #[test]
    fn is_configured_at_platform_config_paths() {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("home/.zone/config.toml");
        let android = root
            .path()
            .join("data/user/0/com.abnegate.zone/files/config.toml");
        let ios = root
            .path()
            .join("Library/Application Support/com.abnegate.zone/config.toml");
        for path in [&desktop, &android, &ios] {
            assert!(!is_configured_at(path));
            write_host_to(path, "https://zone.example.com").unwrap();
            assert!(is_configured_at(path));
            assert_eq!(
                configured_host_from(path).as_deref(),
                Some("https://zone.example.com")
            );
        }
    }

    #[test]
    fn write_host_rejects_parent_dir_components() {
        let err = write_host_to(
            Path::new("/tmp/zone/../config.toml"),
            "https://zone.example.com",
        )
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_host_preserves_other_keys() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        fs::write(
            &path,
            "model = \"gpt-4o\"\nhost = \"https://old.example\"\n",
        )
        .unwrap();
        write_host_to(&path, "https://zone.example.com").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("model = \"gpt-4o\""));
        assert!(content.contains("host = \"https://zone.example.com\""));
        assert!(!content.contains("https://old.example"));
    }
}

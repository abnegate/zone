//! Platform-contract tests for the Tauri Android/iOS client.
//!
//! Native projects live in `gen/` (gitignored). These tests lock the committed
//! config and the Android cleartext patch the Makefile runs after `android init`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("runner/zone_installer -> repo root")
        .to_path_buf()
}

fn desktop_dir() -> PathBuf {
    repo_root().join("runner/zone_desktop")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn tauri_bundle_targets_every_platform() {
    let conf = read_json(&desktop_dir().join("tauri.conf.json"));
    assert_eq!(conf["identifier"], "com.abnegate.zone");
    assert_eq!(conf["productName"], "Zone");
    assert_eq!(conf["bundle"]["android"]["minSdkVersion"], 24);
    assert_eq!(conf["bundle"]["iOS"]["minimumSystemVersion"], "14.0");
    assert_eq!(conf["bundle"]["macOS"]["minimumSystemVersion"], "13.0");
    let targets = conf["bundle"]["targets"]
        .as_array()
        .expect("desktop bundle targets");
    for target in ["app", "dmg", "deb"] {
        assert!(
            targets.iter().any(|value| value == target),
            "missing desktop target {target}"
        );
    }
}

#[test]
fn android_and_ios_overlay_configs() {
    let android = read_json(&desktop_dir().join("tauri.android.conf.json"));
    let ios = read_json(&desktop_dir().join("tauri.ios.conf.json"));
    assert_eq!(android["bundle"]["android"]["minSdkVersion"], 24);
    assert_eq!(ios["bundle"]["iOS"]["minimumSystemVersion"], "14.0");
}

#[test]
fn patch_script_requires_initialized_android_project() {
    let script = repo_root().join("scripts/patch-tauri-android.sh");
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .output()
        .expect("run patch script");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Android project is not initialized"),
        "{stderr}"
    );
}

#[test]
fn patch_script_allows_localhost_cleartext() {
    let script = repo_root().join("scripts/patch-tauri-android.sh");
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("app/src/main");
    fs::create_dir_all(&main).unwrap();
    fs::write(
        main.join("AndroidManifest.xml"),
        r#"<manifest>
    <application android:usesCleartextTraffic="${usesCleartextTraffic}">
    </application>
</manifest>
"#,
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .status()
        .expect("run patch script");
    assert!(status.success());

    let config = fs::read_to_string(main.join("res/xml/network_security_config.xml")).unwrap();
    assert!(config.contains("cleartextTrafficPermitted=\"true\""));
    assert!(config.contains("127.0.0.1"));
    assert!(config.contains("localhost"));

    let manifest = fs::read_to_string(main.join("AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android:networkSecurityConfig=\"@xml/network_security_config\""));
    assert!(manifest.contains("android:usesCleartextTraffic=\"true\""));
}

#[test]
fn patch_script_inserts_application_attributes_when_missing() {
    let script = repo_root().join("scripts/patch-tauri-android.sh");
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("app/src/main");
    fs::create_dir_all(&main).unwrap();
    fs::write(
        main.join("AndroidManifest.xml"),
        "<manifest>\n    <application>\n    </application>\n</manifest>\n",
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .status()
        .expect("run patch script");
    assert!(status.success());

    let manifest = fs::read_to_string(main.join("AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android:networkSecurityConfig=\"@xml/network_security_config\""));
    assert!(manifest.contains("android:usesCleartextTraffic=\"true\""));
}

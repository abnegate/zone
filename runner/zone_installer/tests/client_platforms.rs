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
fn client_webview_uses_stable_loopback() {
    assert_eq!(zone_installer::WEBVIEW_BIND, "127.0.0.1:24727");
    let lib = fs::read_to_string(desktop_dir().join("src/lib.rs")).unwrap();
    assert!(lib.contains("WEBVIEW_BIND"));
    assert!(
        !lib.contains("127.0.0.1:0"),
        "random loopback port drops WebView localStorage (JWT) on every launch"
    );
    assert!(
        lib.contains("get_webview_window(\"main\")"),
        "resume must reuse the existing WebView instead of opening / again"
    );
    let makefile = fs::read_to_string(repo_root().join("Makefile")).unwrap();
    assert!(
        makefile.contains("VITE_DISABLE_PWA=1"),
        "the Tauri SPA must not register a service worker that reloads on resume"
    );
    let vite = fs::read_to_string(repo_root().join("manager/frontend/vite.config.ts")).unwrap();
    assert!(vite.contains("VITE_DISABLE_PWA"));
}

#[test]
fn launcher_icon_is_theme_blue_and_inset() {
    let svg = fs::read_to_string(desktop_dir().join("icons/icon.svg")).unwrap();
    assert!(svg.contains("#0011d9"), "launcher Z must use --ui-accent");
    assert!(
        svg.contains("#1a1612"),
        "launcher background must match the dark canvas"
    );
    assert!(
        svg.contains("translate(21 21) scale(1.03125)"),
        "Z must sit in the Android adaptive 66dp safe zone of 108: {svg}"
    );
    let night = fs::read_to_string(desktop_dir().join("icons/icon-night.svg")).unwrap();
    assert!(
        night.contains("#00f3ff"),
        "night launcher Z must use dark --ui-accent"
    );
    assert!(night.contains("translate(21 21) scale(1.03125)"));
    let script = fs::read_to_string(repo_root().join("scripts/generate-tauri-icons.sh")).unwrap();
    assert!(script.contains("icons/icon.svg"));
    assert!(script.contains("icon-night.svg"));
    assert!(script.contains("mipmap-night-"));
    assert!(!script.contains("favicon.svg"));
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
    assert!(config.contains("<base-config cleartextTrafficPermitted=\"true\">"));
    assert!(config.contains("127.0.0.1"));
    assert!(config.contains("localhost"));
    assert!(config.contains("10.0.2.2"));

    let manifest = fs::read_to_string(main.join("AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android:networkSecurityConfig=\"@xml/network_security_config\""));
    assert!(manifest.contains("android:usesCleartextTraffic=\"true\""));
}

#[test]
fn patch_script_injects_safe_area_insets_into_main_activity() {
    let script = repo_root().join("scripts/patch-tauri-android.sh");
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("app/src/main");
    let activity_dir = main.join("java/com/abnegate/zone");
    fs::create_dir_all(&activity_dir).unwrap();
    fs::write(
        main.join("AndroidManifest.xml"),
        "<manifest>\n    <application>\n    </application>\n</manifest>\n",
    )
    .unwrap();
    fs::write(
        activity_dir.join("MainActivity.kt"),
        "package com.abnegate.zone\nclass MainActivity : TauriActivity()\n",
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .status()
        .expect("run patch script");
    assert!(status.success());

    let activity = fs::read_to_string(activity_dir.join("MainActivity.kt")).unwrap();
    assert!(activity.contains("enableEdgeToEdge()"));
    assert!(activity.contains("WindowInsetsCompat.Type.systemBars()"));
    assert!(activity.contains("Type.displayCutout()"));
    assert!(activity.contains("setProperty('--ui-safe-top'"));
    assert!(activity.contains("onWebViewCreate"));
    assert!(activity.contains("restoreState"));
    assert!(activity.contains("saveState"));
    assert!(activity.contains("onPause"));
    assert!(activity.contains("__zonePersistCurrentPath"));
    assert!(activity.contains("setRendererPriorityPolicy"));
    assert!(activity.contains("RENDERER_PRIORITY_BOUND"));
    assert!(
        activity.contains("webView?.onResume()"),
        "HOME must keep the WebView running; wry onPause invites a renderer kill"
    );
    assert!(activity.contains("stopLoading"));
    assert!(activity.contains("LAST_HREF"));
    assert!(
        activity.contains("putString(LAST_HREF, href).commit()"),
        "native last href must flush before the process is frozen"
    );
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
    assert!(manifest.contains("android:alwaysRetainTaskState=\"true\""));
}

#[test]
fn patch_script_stamps_favicon_launcher_icons() {
    let icons = desktop_dir().join("icons/android");
    let source = icons.join("mipmap-xxxhdpi/ic_launcher.png");
    let night = icons.join("mipmap-night-xxxhdpi/ic_launcher_foreground.png");
    assert!(
        source.is_file(),
        "committed Android icons missing: {source:?}"
    );
    assert!(
        night.is_file(),
        "committed night Android icons missing: {night:?}"
    );
    let background = fs::read_to_string(icons.join("values/ic_launcher_background.xml")).unwrap();
    assert!(
        background.contains("#1A1612"),
        "launcher background must match favicon #1a1612, got {background}"
    );

    let script = repo_root().join("scripts/patch-tauri-android.sh");
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("app/src/main");
    let res = main.join("res");
    fs::create_dir_all(res.join("mipmap-xxxhdpi")).unwrap();
    fs::create_dir_all(res.join("drawable-v24")).unwrap();
    fs::write(
        main.join("AndroidManifest.xml"),
        r#"<manifest>
    <application android:icon="@mipmap/ic_launcher">
    </application>
</manifest>
"#,
    )
    .unwrap();
    fs::write(res.join("mipmap-xxxhdpi/ic_launcher.png"), b"placeholder").unwrap();
    fs::write(
        res.join("drawable-v24/ic_launcher_foreground.xml"),
        "<vector/>\n",
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .status()
        .expect("run patch script");
    assert!(status.success());

    let manifest = fs::read_to_string(main.join("AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android:roundIcon=\"@mipmap/ic_launcher_round\""));

    let stamped = fs::read(res.join("mipmap-xxxhdpi/ic_launcher.png")).unwrap();
    assert_eq!(stamped, fs::read(&source).unwrap());
    let stamped_night =
        fs::read(res.join("mipmap-night-xxxhdpi/ic_launcher_foreground.png")).unwrap();
    assert_eq!(stamped_night, fs::read(&night).unwrap());
    assert!(res.join("mipmap-anydpi-v26/ic_launcher.xml").is_file());
    assert!(!res.join("drawable-v24/ic_launcher_foreground.xml").exists());
    let color = fs::read_to_string(res.join("values/ic_launcher_background.xml")).unwrap();
    assert!(color.contains("#1A1612"), "{color}");
}

#[test]
fn ios_patch_script_requires_initialized_project() {
    let script = repo_root().join("scripts/patch-tauri-ios.sh");
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new("bash")
        .arg(&script)
        .arg(tmp.path().join("missing"))
        .output()
        .expect("run ios patch script");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("iOS project is not initialized"),
        "{stderr}"
    );
}

#[test]
fn ios_patch_script_allows_local_network_and_cleartext() {
    let script = repo_root().join("scripts/patch-tauri-ios.sh");
    let tmp = tempfile::tempdir().unwrap();
    let plist_dir = tmp.path().join("Sources/zone-desktop");
    fs::create_dir_all(&plist_dir).unwrap();
    fs::write(
        plist_dir.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.abnegate.zone</string>
</dict>
</plist>
"#,
    )
    .unwrap();

    let status = Command::new("bash")
        .arg(&script)
        .arg(tmp.path())
        .status()
        .expect("run ios patch script");
    assert!(status.success());

    let plist = fs::read_to_string(plist_dir.join("Info.plist")).unwrap();
    assert!(plist.contains("NSLocalNetworkUsageDescription"));
    assert!(plist.contains("Zone connects to your Zone server on this network."));
    assert!(plist.contains("NSAllowsLocalNetworking"));
    assert!(plist.contains("NSAllowsArbitraryLoads"));
}

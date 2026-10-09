#!/usr/bin/env bash
# Allow the embedded localhost server to load over HTTP on Android.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
android_root="${1:-$root/runner/zone_desktop/gen/android}"
manifest="$android_root/app/src/main/AndroidManifest.xml"
res_dir="$android_root/app/src/main/res/xml"
config="$res_dir/network_security_config.xml"

if [[ ! -f $manifest ]]; then
  echo "Android project is not initialized. Run make android-init first." >&2
  exit 1
fi

mkdir -p "$res_dir"
cat >"$config" <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
    <!-- The WebView talks to the in-app proxy on loopback; the proxy talks
         HTTP to a LAN, emulator, or Tailscale origin the user typed. NSC has
         no CIDR match, so cleartext is allowed for the process. -->
    <base-config cleartextTrafficPermitted="true">
        <trust-anchors>
            <certificates src="system" />
        </trust-anchors>
    </base-config>
    <domain-config cleartextTrafficPermitted="true">
        <domain includeSubdomains="true">127.0.0.1</domain>
        <domain includeSubdomains="true">localhost</domain>
        <domain includeSubdomains="true">10.0.2.2</domain>
    </domain-config>
</network-security-config>
EOF

python3 - "$manifest" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
attr = 'android:networkSecurityConfig="@xml/network_security_config"'
if attr not in text:
    text = text.replace(
        "android:usesCleartextTraffic=\"${usesCleartextTraffic}\"",
        f'{attr}\n        android:usesCleartextTraffic="true"',
        1,
    )
    if attr not in text:
        text = text.replace(
            "<application",
            f"<application\n        {attr}\n        android:usesCleartextTraffic=\"true\"",
            1,
        )
if 'android:alwaysRetainTaskState' not in text:
    text = text.replace(
        "<application",
        '<application\n        android:alwaysRetainTaskState="true"',
        1,
    )
if 'android:roundIcon' not in text:
    text = text.replace(
        'android:icon="@mipmap/ic_launcher"',
        'android:icon="@mipmap/ic_launcher"\n        android:roundIcon="@mipmap/ic_launcher_round"',
        1,
    )
path.write_text(text)
print(f"patched {path}")
PY

python3 - "$android_root" "$root/runner/zone_desktop/icons/android" <<'PY'
from pathlib import Path
import shutil
import sys

root = Path(sys.argv[1])
icons = Path(sys.argv[2])
res = root / "app/src/main/res"
if not res.is_dir() or not icons.is_dir():
    raise SystemExit(0)

for src in icons.rglob("*"):
    if not src.is_file():
        continue
    dest = res / src.relative_to(icons)
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)
    print(f"patched {dest}")

robot = res / "drawable-v24/ic_launcher_foreground.xml"
if robot.is_file():
    robot.unlink()
    print(f"removed {robot}")
PY

python3 - "$android_root" "$(cd "$(dirname "$0")" && pwd)/tauri-android-main-activity.kt" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
source = Path(sys.argv[2]).read_text()
activities = list(root.rglob("MainActivity.kt"))
for path in activities:
    path.write_text(source)
    print(f"patched {path}")
PY

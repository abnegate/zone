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
    <domain-config cleartextTrafficPermitted="true">
        <domain includeSubdomains="true">127.0.0.1</domain>
        <domain includeSubdomains="true">localhost</domain>
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
path.write_text(text)
print(f"patched {path}")
PY

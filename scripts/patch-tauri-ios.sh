#!/usr/bin/env bash
# Allow the Zone client to reach a LAN or Tailscale server from iOS.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
apple_root="${1:-$root/runner/zone_desktop/gen/apple}"

python3 - "$apple_root" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
if not root.is_dir():
    raise SystemExit("iOS project is not initialized. Run make ios-init first.")

plists = [path for path in root.rglob("Info.plist") if path.is_file()]
if not plists:
    raise SystemExit("iOS project is not initialized. Run make ios-init first.")

description = "Zone connects to your Zone server on this network."
insert = f"""    <key>NSLocalNetworkUsageDescription</key>
    <string>{description}</string>
    <key>NSAppTransportSecurity</key>
    <dict>
        <key>NSAllowsLocalNetworking</key>
        <true/>
        <key>NSAllowsArbitraryLoads</key>
        <true/>
    </dict>
"""

for path in plists:
    text = path.read_text()
    if "NSLocalNetworkUsageDescription" not in text:
        marker = "</dict>\n</plist>"
        if marker not in text:
            raise SystemExit(f"unrecognized Info.plist shape: {path}")
        text = text.replace(marker, insert + marker, 1)
        path.write_text(text)
    print(f"patched {path}")
PY

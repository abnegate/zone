#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
envfile="$root/.env.example"
if [ ! -f "$envfile" ]; then
    printf '%s\n' 'Missing .env.example' >&2
    exit 1
fi

compose() {
    docker compose --env-file "$envfile" "$@"
}

direct=$(mktemp)
vpn=$(mktemp)
trap 'rm -f "$direct" "$vpn"' EXIT HUP INT TERM

compose -f "$root/docker-compose.yml" config --format json > "$direct"
compose -f "$root/docker-compose.yml" -f "$root/docker-compose.vpn.yml" \
    --profile vpn --profile monitoring --profile bundled-ollama \
    --profile bundled-comfyui --profile comfyui-model-setup \
    config --format json > "$vpn"

python3 - "$direct" "$vpn" <<'PY'
import json
import sys

direct = json.load(open(sys.argv[1], encoding="utf-8"))
vpn = json.load(open(sys.argv[2], encoding="utf-8"))

direct_manager = direct["services"]["manager"]
if direct_manager.get("network_mode"):
    raise SystemExit("default compose must not attach manager to Gluetun")
if not direct_manager.get("networks"):
    raise SystemExit("default compose manager must keep Docker networks")

attached = (
    "manager",
    "litellm",
    "grafana",
    "ollama",
    "ollama-init",
    "comfyui",
    "comfyui-model-setup",
    "searxng",
)
for name in attached:
    service = vpn["services"][name]
    if service.get("network_mode") != "service:gluetun":
        raise SystemExit(f"{name} must use network_mode service:gluetun when VPN is on")
    if service.get("networks"):
        raise SystemExit(f"{name} must not keep a Docker network when VPN is on")

manager_env = vpn["services"]["manager"].get("environment") or {}
if manager_env.get("LITELLM_HOST") != "http://127.0.0.1:4000":
    raise SystemExit("VPN manager must reach LiteLLM on localhost")

gluetun = vpn["services"]["gluetun"]
aliases = set()
for network in (gluetun.get("networks") or {}).values():
    aliases.update((network or {}).get("aliases") or [])
for name in ("manager", "litellm", "grafana"):
    if name not in aliases:
        raise SystemExit(f"gluetun must alias {name} on Docker networks")

labels = gluetun.get("labels") or {}
if isinstance(labels, list):
    labels = dict(item.split("=", 1) for item in labels)
if labels.get("traefik.http.services.manager.loadbalancer.server.port") != "8000":
    raise SystemExit("gluetun must publish manager to Traefik when VPN is on")

print("VPN compose overlay checks passed")
PY

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
if manager_env.get("MODEL_SEARCH_PROXY_URL") != "http://127.0.0.1:8888":
    raise SystemExit(
        "VPN manager must reach the Gluetun HTTP proxy on localhost "
        "(gluetun hostname does not resolve in the shared namespace)"
    )
if manager_env.get("TOOL_RUNNER_PROXY_URL") != "http://127.0.0.1:8888":
    raise SystemExit("VPN manager tool proxy must use localhost, not gluetun")

gluetun = vpn["services"]["gluetun"]
aliases = set()
for network in (gluetun.get("networks") or {}).values():
    aliases.update((network or {}).get("aliases") or [])
for name in ("manager", "litellm", "grafana"):
    if name not in aliases:
        raise SystemExit(f"gluetun must alias {name} on Docker networks")

# Gluetun DoT does not resolve Docker names, and extra_hosts cannot be set on
# network_mode=service containers. Pin internal IPs and point VPN-attached
# services at those addresses.
pinned = {
    "postgres": "172.30.0.20",
    "valkey": "172.30.0.21",
    "prometheus": "172.30.0.22",
}


def extra_hosts_map(service):
    hosts = service.get("extra_hosts") or {}
    if isinstance(hosts, dict):
        return {str(key): str(value) for key, value in hosts.items()}
    mapped = {}
    for item in hosts:
        text = str(item)
        if "=" in text:
            host, address = text.split("=", 1)
        elif ":" in text:
            host, address = text.split(":", 1)
        else:
            continue
        mapped[host] = address
    return mapped


def ipv4_address(service, network):
    networks = service.get("networks") or {}
    if isinstance(networks, dict):
        return (networks.get(network) or {}).get("ipv4_address")
    return None


def service_env(service):
    env = service.get("environment") or {}
    if isinstance(env, dict):
        return {str(key): "" if value is None else str(value) for key, value in env.items()}
    mapped = {}
    for item in env:
        key, sep, value = str(item).partition("=")
        if sep:
            mapped[key] = value
    return mapped


for name, address in pinned.items():
    if ipv4_address(vpn["services"][name], "internal") != address:
        raise SystemExit(f"{name} must pin {address} on the internal network")

gluetun_hosts = extra_hosts_map(gluetun)
if gluetun_hosts.get("gluetun") != "127.0.0.1":
    raise SystemExit(
        "gluetun must extra_hosts gluetun=127.0.0.1 so VPN-attached processes "
        "can resolve the proxy hostname"
    )
for host, address in pinned.items():
    if gluetun_hosts.get(host) != address:
        raise SystemExit(
            f"gluetun must extra_hosts {host}={address} (got {gluetun_hosts.get(host)!r})"
        )

for name in ("litellm", "manager", "grafana"):
    if extra_hosts_map(vpn["services"][name]):
        raise SystemExit(
            f"{name} cannot set extra_hosts with network_mode=service:gluetun"
        )

litellm_env = service_env(vpn["services"]["litellm"])
if litellm_env.get("POSTGRES_HOST") != pinned["postgres"]:
    raise SystemExit("VPN LiteLLM must use the pinned Postgres address")

manager_env = service_env(vpn["services"]["manager"])
if pinned["postgres"] not in manager_env.get("DATABASE_URL", ""):
    raise SystemExit("VPN manager DATABASE_URL must use the pinned Postgres address")
if pinned["valkey"] not in manager_env.get("REDIS_URL", ""):
    raise SystemExit("VPN manager REDIS_URL must use the pinned Valkey address")
if pinned["prometheus"] not in manager_env.get("MONITORING_PROMETHEUS_URL", ""):
    raise SystemExit("VPN manager must scrape Prometheus on the pinned address")
if manager_env.get("MODEL_SEARCH_PROXY_URL") != "http://127.0.0.1:8888":
    raise SystemExit("VPN manager MODEL_SEARCH_PROXY_URL must be loopback")
if manager_env.get("TOOL_RUNNER_PROXY_URL") != "http://127.0.0.1:8888":
    raise SystemExit("VPN manager TOOL_RUNNER_PROXY_URL must be loopback")

grafana_env = service_env(vpn["services"]["grafana"])
if grafana_env.get("PROMETHEUS_URL") != f"http://{pinned['prometheus']}:9090":
    raise SystemExit("VPN Grafana must use the pinned Prometheus address")

labels = gluetun.get("labels") or {}
if isinstance(labels, list):
    labels = dict(item.split("=", 1) for item in labels)
if labels.get("traefik.http.services.manager.loadbalancer.server.port") != "8000":
    raise SystemExit("gluetun must publish manager to Traefik when VPN is on")

gluetun_env = service_env(gluetun)
if gluetun_env.get("HTTP_CONTROL_SERVER_ADDRESS") != "127.0.0.1:8007":
    raise SystemExit("gluetun control API must move off :8000 so manager can bind")

print("VPN compose overlay checks passed")
PY

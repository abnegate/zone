#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
script=$root/scripts/compose.sh
envfile=$root/.env.example

compose() {
    docker compose --env-file "$envfile" "$@"
}

trim() {
    printf '%s' "$1" | sed 's/[[:space:]]*$//'
}

assert_eq() {
    actual=$1
    expected=$2
    message=$3
    if [ "$actual" != "$expected" ]; then
        printf '%s\n expected: %s\n      got: %s\n' "$message" "$expected" "$actual" >&2
        exit 1
    fi
}

assert_contains() {
    haystack=$1
    needle=$2
    message=$3
    case "$haystack" in
        *"$needle"*) ;;
        *)
            printf '%s\n missing %s in:\n%s\n' "$message" "$needle" "$haystack" >&2
            exit 1
            ;;
    esac
}

assert_not_contains() {
    haystack=$1
    needle=$2
    message=$3
    case "$haystack" in
        *"$needle"*)
            printf '%s\n unexpectedly found %s in:\n%s\n' "$message" "$needle" "$haystack" >&2
            exit 1
            ;;
    esac
}

flags_core=$(trim "$("$script" flags '')")
assert_eq "$flags_core" '-f docker-compose.yml' 'core flags'

flags_combo=$(trim "$("$script" flags 'monitoring,vpn,dev')")
assert_eq "$flags_combo" \
    '-f docker-compose.yml -f docker-compose.dev.yml -f docker-compose.vpn.yml --profile dev --profile vpn --profile monitoring' \
    'combined flags'

flags_monitoring=$(trim "$("$script" flags monitoring)")
assert_eq "$flags_monitoring" \
    '-f docker-compose.yml --profile monitoring' \
    'monitoring does not select overlays'

assert_eq "$("$script" normalize 'monitoring,dev,vpn,dev')" 'dev,vpn,monitoring' 'normalize order and dedupe'

directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT HUP INT TERM
cp "$envfile" "$directory/environment"
unset COMPOSE_PROFILES COMPOSE_FILE ZONE_VPN MODEL_SEARCH_PROXY_URL TOOL_RUNNER_PROXY_URL COMPOSE_PATH_SEPARATOR

persisted=$("$script" persist --env-file "$directory/environment" 'dev,vpn,monitoring')
assert_eq "$persisted" 'dev,vpn,monitoring' 'persist prints normalized profiles'
assert_contains "$(cat "$directory/environment")" 'COMPOSE_PROFILES=dev,vpn,monitoring' 'persist COMPOSE_PROFILES'
assert_contains "$(cat "$directory/environment")" \
    'COMPOSE_FILE=docker-compose.yml:docker-compose.dev.yml:docker-compose.vpn.yml' \
    'persist COMPOSE_FILE'
assert_contains "$(cat "$directory/environment")" 'ZONE_VPN=1' 'persist enables ZONE_VPN for vpn'
assert_contains "$(cat "$directory/environment")" 'MODEL_SEARCH_PROXY_URL=http://gluetun:8888' 'persist vpn proxy'

"$script" persist --env-file "$directory/environment" >/dev/null
assert_contains "$(cat "$directory/environment")" 'COMPOSE_PROFILES=' 'persist empty clears profiles'
assert_contains "$(cat "$directory/environment")" 'ZONE_VPN=' 'persist empty clears ZONE_VPN'
if grep -q '^[[:space:]]*COMPOSE_FILE=' "$directory/environment"; then
    printf '%s\n' 'persist empty must remove COMPOSE_FILE' >&2
    exit 1
fi

"$script" persist --env-file "$directory/environment" vpn >/dev/null
ensured=$("$script" persist --env-file "$directory/environment" --ensure dev)
assert_eq "$ensured" 'dev,vpn' 'persist --ensure keeps vpn and adds dev'

# shellcheck disable=SC2046
core_services=$(compose $("$script" flags '') config --services)
assert_not_contains "$core_services" 'gluetun' 'core does not start gluetun'
assert_not_contains "$core_services" 'prometheus' 'core does not start prometheus'
assert_not_contains "$core_services" 'grafana' 'core does not start grafana'
assert_contains "$core_services" 'manager' 'core starts manager'

# shellcheck disable=SC2046
vpn_services=$(compose $("$script" flags vpn) config --services)
assert_contains "$vpn_services" 'gluetun' 'vpn profile starts gluetun'
assert_contains "$vpn_services" 'searxng' 'vpn profile starts searxng'

# shellcheck disable=SC2046
monitoring_services=$(compose $("$script" flags monitoring) config --services)
assert_contains "$monitoring_services" 'prometheus' 'monitoring profile starts prometheus'
assert_contains "$monitoring_services" 'grafana' 'monitoring profile starts grafana'
assert_not_contains "$monitoring_services" 'gluetun' 'monitoring does not start gluetun'

direct=$(mktemp)
devcfg=$(mktemp)
combo=$(mktemp)
bundled=$(mktemp)
moved=$(mktemp)
off=$(mktemp)
tunnel=$(mktemp)
closed=$(mktemp)
trap 'rm -rf "$directory" "$direct" "$devcfg" "$combo" "$bundled" "$moved" "$off" "$tunnel" "$closed"' EXIT HUP INT TERM
unset ZONE_AGENT_CALLBACK_PORT ZONE_CONSOLE_ORIGINS

# shellcheck disable=SC2046
compose $("$script" flags '') config --format json > "$direct"
# shellcheck disable=SC2046
compose $("$script" flags dev) config --format json > "$devcfg"
# shellcheck disable=SC2046
compose $("$script" flags 'dev,vpn,monitoring') config --format json > "$combo"
# shellcheck disable=SC2046
compose $("$script" flags bundled-ollama) config --format json > "$bundled"
# shellcheck disable=SC2046
ZONE_AGENT_CALLBACK_PORT=60000 compose $("$script" flags 'dev,vpn') config --format json > "$moved"
# shellcheck disable=SC2046
ZONE_AGENT_CALLBACK_PORT='' compose $("$script" flags '') config --format json > "$off"
# shellcheck disable=SC2046
ZONE_CONSOLE_ORIGINS=http://manager.localhost:8080 compose $("$script" flags '') config --format json > "$tunnel"
# shellcheck disable=SC2046
ZONE_CONSOLE_ORIGINS='' compose $("$script" flags dev) config --format json > "$closed"

python3 - "$direct" "$devcfg" "$combo" "$bundled" "$moved" "$off" "$tunnel" "$closed" <<'PY'
import json
import sys

direct = json.load(open(sys.argv[1], encoding="utf-8"))
dev = json.load(open(sys.argv[2], encoding="utf-8"))
combo = json.load(open(sys.argv[3], encoding="utf-8"))
bundled = json.load(open(sys.argv[4], encoding="utf-8"))
moved = json.load(open(sys.argv[5], encoding="utf-8"))
off = json.load(open(sys.argv[6], encoding="utf-8"))
tunnel = json.load(open(sys.argv[7], encoding="utf-8"))
closed = json.load(open(sys.argv[8], encoding="utf-8"))


def dockerfile(service):
    build = service.get("build") or {}
    if isinstance(build, str):
        return build
    return build.get("dockerfile") or ""


def published_ports(service):
    ports = []
    for item in service.get("ports") or []:
        if isinstance(item, dict):
            published = item.get("published")
            target = item.get("target")
            ports.append(f"{published}:{target}")
        else:
            ports.append(str(item))
    return ports


direct_manager = direct["services"]["manager"]
if direct_manager.get("network_mode"):
    raise SystemExit("core manager must not attach to Gluetun")
if "Dockerfile.dev" in dockerfile(direct_manager):
    raise SystemExit("core manager must use the production Dockerfile")

dev_manager = dev["services"]["manager"]
if "Dockerfile.dev" not in dockerfile(dev_manager):
    raise SystemExit("dev manager must use Dockerfile.dev")
if "5432:5432" not in published_ports(dev["services"]["postgres"]):
    raise SystemExit("dev postgres must publish 5432")

combo_manager = combo["services"]["manager"]
if combo_manager.get("network_mode") != "service:gluetun":
    raise SystemExit("dev+vpn manager must use network_mode service:gluetun")
if "Dockerfile.dev" not in dockerfile(combo_manager):
    raise SystemExit("dev+vpn manager must keep Dockerfile.dev")
if combo["services"]["grafana"].get("network_mode") != "service:gluetun":
    raise SystemExit("dev+vpn+monitoring grafana must use network_mode service:gluetun")
if "5432:5432" not in published_ports(combo["services"]["postgres"]):
    raise SystemExit("dev+vpn postgres must keep published 5432")

prometheus_profiles = combo["services"]["prometheus"].get("profiles") or []
if prometheus_profiles != ["monitoring"]:
    raise SystemExit(f"prometheus must use the monitoring profile, got {prometheus_profiles!r}")


def volume_targets(service):
    targets = {}
    for item in service.get("volumes") or []:
        if isinstance(item, dict):
            targets[item.get("target")] = item.get("source")
        else:
            parts = str(item).split(":")
            if len(parts) >= 2:
                targets[parts[1]] = parts[0]
    return targets


# The pgvector image declares /var/lib/postgresql/data a VOLUME and keeps
# PGDATA there; a named volume mounted anywhere else leaves the cluster in an
# anonymous volume that make backup never sees.
for name, config in (("core", direct), ("dev", dev), ("dev+vpn+monitoring", combo)):
    targets = volume_targets(config["services"]["postgres"])
    source = targets.get("/var/lib/postgresql/data")
    if source is None:
        raise SystemExit(
            f"{name} postgres must mount its named volume at /var/lib/postgresql/data, "
            f"got {sorted(targets)!r}"
        )
    if "/var/lib/postgresql" in targets:
        raise SystemExit(f"{name} postgres must not also mount /var/lib/postgresql")

# host.docker.internal is unroutable from the internal-only network, so an
# ollama-init pointed at a host Ollama needs the edge network too.
init_networks = set(bundled["services"]["ollama-init"].get("networks") or {})
if not {"internal", "edge"} <= init_networks:
    raise SystemExit(f"ollama-init must join internal and edge, got {sorted(init_networks)!r}")

# claude.com sends the admin's browser to http://localhost:<port>/callback, so
# the published port must answer only the host's own loopback. Docker hands a
# published port to the container's interface, never to its loopback, which is
# why the listener inside binds 0.0.0.0, always on the same port.
LISTENER = 54545


def callback_publishes(service):
    return [
        (item.get("host_ip"), str(item.get("published") or ""))
        for item in service.get("ports") or []
        if isinstance(item, dict) and int(item.get("target")) == LISTENER
    ]


for name, config, publisher, port in (
    ("core", direct, "manager", "54545"),
    ("dev", dev, "manager", "54545"),
    ("dev+vpn+monitoring", combo, "gluetun", "54545"),
    ("dev+vpn with ZONE_AGENT_CALLBACK_PORT=60000", moved, "gluetun", "60000"),
):
    publishes = callback_publishes(config["services"][publisher])
    if publishes != [("127.0.0.1", port)]:
        raise SystemExit(
            f"{name} {publisher} must publish the Claude sign-in callback on 127.0.0.1:{port} "
            f"only, got {publishes!r}"
        )
    environment = config["services"]["manager"].get("environment") or {}
    for variable, value in (
        ("ZONE_AGENT_CALLBACK", port),
        ("ZONE_AGENT_CALLBACK_BIND", f"0.0.0.0:{LISTENER}"),
    ):
        if environment.get(variable) != value:
            raise SystemExit(
                f"{name} manager must set {variable}={value}, got {environment.get(variable)!r}"
            )
    if publisher == "gluetun":
        firewall = str(config["services"]["gluetun"]["environment"].get("FIREWALL_INPUT_PORTS", ""))
        if str(LISTENER) not in firewall.split(","):
            raise SystemExit(f"{name} gluetun must let the callback in, got {firewall!r}")
for name, config in (("dev+vpn+monitoring", combo), ("dev+vpn", moved)):
    if config["services"]["manager"].get("ports"):
        raise SystemExit(f"{name} manager shares gluetun's network, so gluetun publishes its ports")

off_manager = off["services"]["manager"]
if (off_manager.get("environment") or {}).get("ZONE_AGENT_CALLBACK") != "":
    raise SystemExit("an empty ZONE_AGENT_CALLBACK_PORT must turn the callback off")
if any(
    host != "127.0.0.1" or published == str(LISTENER)
    for host, published in callback_publishes(off_manager)
):
    raise SystemExit(
        "with the callback off, the manager must not take port "
        f"{LISTENER}, got {callback_publishes(off_manager)!r}"
    )

# A Claude sign-in returns its browser only to a console the manager lists, so
# the list names exactly the consoles each stack serves: Traefik's, on both
# entrypoints and both hosts, plus the Vite server the dev overlay publishes.
# .env replaces it, and an empty value turns the return off.
TRAEFIK_CONSOLES = (
    "http://manager.localhost,https://manager.localhost,"
    "http://manager.webui.localhost,https://manager.webui.localhost"
)
VITE_CONSOLE = "http://localhost:3001"
for name, config, consoles in (
    ("core", direct, TRAEFIK_CONSOLES),
    ("core with the callback off", off, TRAEFIK_CONSOLES),
    ("dev", dev, f"{VITE_CONSOLE},{TRAEFIK_CONSOLES}"),
    ("dev+vpn+monitoring", combo, f"{VITE_CONSOLE},{TRAEFIK_CONSOLES}"),
    ("dev+vpn with ZONE_AGENT_CALLBACK_PORT=60000", moved, f"{VITE_CONSOLE},{TRAEFIK_CONSOLES}"),
    ("core with an SSH tunnel's console", tunnel, "http://manager.localhost:8080"),
    ("dev with ZONE_CONSOLE_ORIGINS empty", closed, ""),
):
    environment = config["services"]["manager"].get("environment") or {}
    if environment.get("ZONE_CONSOLE_ORIGINS") != consoles:
        raise SystemExit(
            f"{name} manager must set ZONE_CONSOLE_ORIGINS={consoles}, "
            f"got {environment.get('ZONE_CONSOLE_ORIGINS')!r}"
        )

print("Compose profile combination checks passed")
PY

printf '%s\n' 'Compose profile checks passed'

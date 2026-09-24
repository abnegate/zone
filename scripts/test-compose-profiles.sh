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
trap 'rm -rf "$directory" "$direct" "$devcfg" "$combo" "$bundled"' EXIT HUP INT TERM

# shellcheck disable=SC2046
compose $("$script" flags '') config --format json > "$direct"
# shellcheck disable=SC2046
compose $("$script" flags dev) config --format json > "$devcfg"
# shellcheck disable=SC2046
compose $("$script" flags 'dev,vpn,monitoring') config --format json > "$combo"
# shellcheck disable=SC2046
compose $("$script" flags bundled-ollama) config --format json > "$bundled"

python3 - "$direct" "$devcfg" "$combo" "$bundled" <<'PY'
import json
import sys

direct = json.load(open(sys.argv[1], encoding="utf-8"))
dev = json.load(open(sys.argv[2], encoding="utf-8"))
combo = json.load(open(sys.argv[3], encoding="utf-8"))
bundled = json.load(open(sys.argv[4], encoding="utf-8"))


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

# claude.com sends the admin's browser to http://localhost:54545/callback, so
# the published port must answer only the host's own loopback. Docker hands a
# published port to the container's interface, never to its loopback, which is
# why the listener inside binds 0.0.0.0.
CALLBACK = 54545


def callback_hosts(service):
    return [
        item.get("host_ip")
        for item in service.get("ports") or []
        if isinstance(item, dict)
        and str(item.get("published")) == str(CALLBACK)
        and int(item.get("target")) == CALLBACK
    ]


for name, config, publisher in (
    ("core", direct, "manager"),
    ("dev", dev, "manager"),
    ("dev+vpn+monitoring", combo, "gluetun"),
):
    hosts = callback_hosts(config["services"][publisher])
    if hosts != ["127.0.0.1"]:
        raise SystemExit(
            f"{name} {publisher} must publish the Claude sign-in callback on 127.0.0.1 only, "
            f"got {hosts!r}"
        )
    environment = config["services"]["manager"].get("environment") or {}
    for variable, value in (
        ("ZONE_AGENT_CALLBACK", f"http://localhost:{CALLBACK}"),
        ("ZONE_AGENT_CALLBACK_BIND", "0.0.0.0"),
    ):
        if environment.get(variable) != value:
            raise SystemExit(
                f"{name} manager must set {variable}={value}, got {environment.get(variable)!r}"
            )
if combo["services"]["manager"].get("ports"):
    raise SystemExit("dev+vpn manager shares gluetun's network, so gluetun publishes its ports")
firewall = str(combo["services"]["gluetun"]["environment"].get("FIREWALL_INPUT_PORTS", ""))
if str(CALLBACK) not in firewall.split(","):
    raise SystemExit(f"gluetun must let the callback in through its firewall, got {firewall!r}")

print("Compose profile combination checks passed")
PY

printf '%s\n' 'Compose profile checks passed'

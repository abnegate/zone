#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
chart=$root/helm/zone-apps
warning='server.trustedProxies is empty'

render() {
    helm template zone-apps "$chart" --namespace zone \
        --show-only templates/server/deployment.yaml "$@"
}

notes() {
    helm install zone-apps "$chart" --namespace zone --dry-run=client "$@" |
        sed -n '/^NOTES:/,$p'
}

trusted() {
    printf '%s\n' "$1" | awk '
        $1 == "-" && $2 == "name:" && $3 == "ZONE_TRUSTED_PROXIES" {
            getline
            sub(/^[[:space:]]*value:[[:space:]]*/, "")
            gsub(/"/, "")
            print
        }
    '
}

assert_trusted() {
    expected=$1
    message=$2
    shift 2
    rendered=$(render "$@")
    actual=$(trusted "$rendered")
    if [ "$actual" != "$expected" ]; then
        printf '%s\n expected: %s\n      got: %s\n' "$message" "$expected" "$actual" >&2
        exit 1
    fi
}

assert_warned() {
    message=$1
    shift
    case "$(notes "$@")" in
        *"$warning"*) ;;
        *)
            printf '%s\n the notes do not warn that %s\n' "$message" "$warning" >&2
            exit 1
            ;;
    esac
}

assert_quiet() {
    message=$1
    shift
    case "$(notes "$@")" in
        *"$warning"*)
            printf '%s\n the notes warn that %s\n' "$message" "$warning" >&2
            exit 1
            ;;
        *) ;;
    esac
}

assert_loopback() {
    message=$1
    shift
    rendered=$(notes "$@")
    case "$rendered" in
        *'trusts no proxy'*)
            printf '%s\n the notes say the server trusts no proxy\n' "$message" >&2
            exit 1
            ;;
        *loopback*) ;;
        *)
            printf '%s\n the notes do not say the server trusts loopback\n' "$message" >&2
            exit 1
            ;;
    esac
}

assert_trusted '' \
    'by default the server is not told to trust any proxy, so it trusts loopback only'

assert_warned 'an install that trusts only loopback is warned about it'

assert_loopback 'the warning says the server trusts loopback, not that it trusts no proxy'

assert_trusted '10.244.0.0/16' \
    'server.trustedProxies is passed to the server' \
    --set server.trustedProxies=10.244.0.0/16

assert_trusted '10.244.0.0/16,fd00:10:244::/56' \
    'a list in server.trustedProxies is passed whole and trimmed' \
    --set-string 'server.trustedProxies= 10.244.0.0/16\,fd00:10:244::/56 '

assert_quiet 'an install with a trusted proxy is not warned' \
    --set server.trustedProxies=10.244.0.0/16

assert_trusted '10.42.0.0/16' \
    'server.env.ZONE_TRUSTED_PROXIES wins over server.trustedProxies and is rendered once' \
    --set server.trustedProxies=10.244.0.0/16 \
    --set server.env.ZONE_TRUSTED_PROXIES=10.42.0.0/16

assert_quiet 'server.env.ZONE_TRUSTED_PROXIES alone silences the warning' \
    --set server.env.ZONE_TRUSTED_PROXIES=10.42.0.0/16

assert_trusted '' \
    'an empty server.env.ZONE_TRUSTED_PROXIES counts as unset' \
    --set server.env.ZONE_TRUSTED_PROXIES=

assert_trusted '' \
    'a blank server.trustedProxies counts as unset' \
    --set-string 'server.trustedProxies= '

assert_warned 'a blank server.trustedProxies is warned about' \
    --set-string 'server.trustedProxies= '

render_ha() {
    helm template zone-apps "$chart" --namespace zone \
        -f "$chart/values-ha.yaml" \
        --set highAvailability.externalValkey=true \
        --set server.agentState.persistence.storageClass=test-rwx "$@"
}

isolated() {
    render_ha "$@" \
        --set networkPolicy.enabled=true \
        --set networkPolicy.externalValkeyCIDR=10.20.0.0/16 \
        --set networkPolicy.externalValkeyPort=6379
}

assert_ha_refused() {
    message=$1
    reason=$2
    shift 2
    if refusal=$(render_ha "$@" 2>&1 >/dev/null); then
        printf '%s\n rendered instead of being refused\n' "$message" >&2
        exit 1
    fi
    for expected in "$reason" networkPolicy.enabled highAvailability.externalNetworkPolicy; do
        case "$refusal" in
            *"$expected"*) ;;
            *)
                printf '%s\n refused without "%s":\n%s\n' "$message" "$expected" "$refusal" >&2
                exit 1
                ;;
        esac
    done
}

assert_ha_renders() {
    message=$1
    shift
    if ! output=$("$@" 2>&1 >/dev/null); then
        printf '%s\n the render was refused:\n%s\n' "$message" "$output" >&2
        exit 1
    fi
}

assert_ha_refused 'HA trusting a pod network with no network policy is refused' \
    'trusts the network 10.244.0.0/16' \
    --set server.trustedProxies=10.244.0.0/16

assert_ha_refused 'HA trusting a network through server.env is refused' \
    'trusts the network 10.42.0.0/16' \
    --set server.env.ZONE_TRUSTED_PROXIES=10.42.0.0/16

assert_ha_refused 'HA trusting an IPv6 network is refused' \
    'trusts the network fd00:10:244::/56' \
    --set-string 'server.trustedProxies=127.0.0.1/32\,fd00:10:244::/56'

assert_ha_refused 'HA naming only the networks in a list of hosts and networks' \
    'trusts the network 10.0.0.0/8, 172.31.0.0/24' \
    --set-string 'server.trustedProxies=10.0.0.0/8\, 10.244.1.7/32\,172.31.0.0/24'

assert_ha_refused 'HA trusting every address is refused' \
    'trusts the network 0.0.0.0/0' \
    --set server.trustedProxies=0.0.0.0/0

assert_ha_renders 'HA trusting a pod network renders with the chart network policy' \
    isolated --set server.trustedProxies=10.244.0.0/16

assert_ha_renders 'HA trusting a pod network renders with a network policy of your own' \
    render_ha --set server.trustedProxies=10.244.0.0/16 --set highAvailability.externalNetworkPolicy=true

assert_ha_renders 'HA trusting loopback only renders without a network policy' \
    render_ha --set-string 'server.trustedProxies=127.0.0.1/32\,::1/128'

assert_ha_renders 'HA trusting the loopback network renders without a network policy' \
    render_ha --set server.trustedProxies=127.0.0.0/8

assert_ha_renders 'HA trusting single addresses renders without a network policy' \
    render_ha --set-string 'server.trustedProxies=10.244.1.7\,10.244.1.8/32\,fd00::8/128\,198.51.100.7:4711'

assert_trusted '10.244.0.0/16' \
    'outside HA a trusted network needs no network policy' \
    --set server.trustedProxies=10.244.0.0/16

policies() {
    helm template zone-apps "$chart" --namespace zone \
        --show-only templates/networkpolicy.yaml --set networkPolicy.enabled=true "$@"
}

assert_policy() {
    message=$1
    expected=$2
    expression=$3
    shift 3
    actual=$(policies "$@" | yq eval-all "$expression" -)
    if [ "$actual" != "$expected" ]; then
        printf '%s\n expected: %s\n      got: %s\n' "$message" "$expected" "$actual" >&2
        exit 1
    fi
}

assert_policy "the default-deny policy selects only this release's pods, so the Postgres, Valkey and LiteLLM pods beside it keep their traffic" \
    'app.kubernetes.io/name=zone-apps,app.kubernetes.io/instance=zone-apps' \
    'select(.metadata.name == "zone-apps-default-deny") | .spec.podSelector.matchLabels | to_entries | map(.key + "=" + .value) | join(",")'

assert_policy 'the ingress controller is admitted on the ports the server and console pods listen on' \
    '8000 3001' \
    '[select(.metadata.name == "zone-apps-server-ingress" or .metadata.name == "zone-apps-manager-ingress") | .spec.ingress[0].ports[0].port] | join(" ")' \
    --set server.service.port=80 --set manager.service.port=81

printf '%s\n' 'Helm trusted proxy checks passed'

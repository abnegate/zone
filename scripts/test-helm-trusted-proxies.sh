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

assert_trusted '' \
    'by default the server is not told to trust any proxy, so it trusts loopback only'

assert_warned 'an install that trusts no proxy is warned about it'

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

printf '%s\n' 'Helm trusted proxy checks passed'

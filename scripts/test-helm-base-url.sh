#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
chart=$root/helm/zone-apps

render() {
    helm template zone-apps "$chart" --namespace zone \
        --show-only templates/server/deployment.yaml "$@"
}

origins() {
    printf '%s\n' "$1" | awk '
        $1 == "-" && $2 == "name:" && $3 == "APP_BASE_URL" {
            getline
            sub(/^[[:space:]]*value:[[:space:]]*/, "")
            gsub(/"/, "")
            print
        }
    '
}

assert_origin() {
    expected=$1
    message=$2
    shift 2
    rendered=$(render "$@")
    actual=$(origins "$rendered")
    if [ "$actual" != "$expected" ]; then
        printf '%s\n expected: %s\n      got: %s\n' "$message" "$expected" "$actual" >&2
        exit 1
    fi
}

assert_refused() {
    message=$1
    shift
    if render "$@" >/dev/null 2>&1; then
        printf '%s\n' "$message" >&2
        exit 1
    fi
}

assert_origin 'http://zone.local' \
    'the server is told the ingress host is its origin'

assert_origin 'https://zone.local' \
    'a TLS entry naming the ingress host makes the origin https' \
    --set-json 'ingress.tls=[{"secretName":"zone-tls","hosts":["zone.local"]}]'

assert_origin 'https://zone.example.com' \
    'a wildcard TLS entry one label up makes the origin https' \
    --set-json 'ingress.hosts=[{"host":"zone.example.com","paths":[]}]' \
    --set-json 'ingress.tls=[{"secretName":"zone-tls","hosts":["*.example.com"]}]'

assert_origin 'https://zone.local' \
    'a TLS entry with no hosts covers every host of the ingress' \
    --set-json 'ingress.tls=[{"secretName":"zone-tls"}]'

assert_origin 'http://zone.local' \
    'a TLS entry for another host leaves the origin http' \
    --set-json 'ingress.tls=[{"secretName":"zone-tls","hosts":["other.local"]}]'

assert_origin 'http://first.example.com' \
    'the first ingress host is the origin' \
    --set-json 'ingress.hosts=[{"host":"first.example.com","paths":[]},{"host":"second.example.com","paths":[]}]'

assert_origin '' \
    'without an ingress there is no origin to derive, so the server keeps its own default' \
    --set ingress.enabled=false

assert_origin 'https://zone.example.com' \
    'server.baseUrl replaces the ingress origin' \
    --set server.baseUrl=https://zone.example.com

assert_origin 'https://zone.example.com' \
    'server.baseUrl is passed without its trailing slash' \
    --set server.baseUrl=https://zone.example.com/

assert_origin 'https://zone.example.com:8443' \
    'server.baseUrl gives an origin to an install with no ingress' \
    --set ingress.enabled=false \
    --set server.baseUrl=https://zone.example.com:8443

assert_origin 'https://env.example.com' \
    'server.env.APP_BASE_URL wins over server.baseUrl and is rendered once' \
    --set server.baseUrl=https://value.example.com \
    --set server.env.APP_BASE_URL=https://env.example.com

assert_origin 'https://common.example.com' \
    'commonEnv.APP_BASE_URL replaces the ingress origin and is rendered once' \
    --set commonEnv.APP_BASE_URL=https://common.example.com

assert_origin 'https://value.example.com' \
    'server.baseUrl wins over commonEnv.APP_BASE_URL and is rendered once' \
    --set commonEnv.APP_BASE_URL=https://common.example.com \
    --set server.baseUrl=https://value.example.com

assert_origin 'http://zone.local' \
    'an empty server.env.APP_BASE_URL counts as unset' \
    --set server.env.APP_BASE_URL=

assert_refused 'server.baseUrl with a path is refused' \
    --set server.baseUrl=https://zone.example.com/console

assert_origin 'https://zone.example.com' \
    'the production example serves TLS, so its origin is https' \
    -f "$chart/examples/production.yaml"

assert_origin 'http://zone.localhost' \
    'the development example has no TLS, so its origin is http' \
    -f "$chart/examples/development.yaml"

assert_origin 'http://localhost:3001' \
    'a Tilt install is opened at the console dev server' \
    -f "$root/k8s/values-tilt-apps.yaml"

printf '%s\n' 'Helm base URL checks passed'

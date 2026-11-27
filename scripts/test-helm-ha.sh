#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
charts=${ZONE_HELM_CHARTS:-$root/helm}
guide=${ZONE_HA_GUIDE:-$root/docs/HA.md}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
storage_class=test-rwx
trusted_proxies=10.244.0.0/16
loopback='127.0.0.1/32,::1/128'
failures=0
shutdown_tail=17
ingress_sync_seconds=5

jwt_secret=0123456789abcdef0123456789abcdef
encryption_key=fedcba9876543210fedcba9876543210

render() {
    chart=$1
    shift
    helm template "$charts/$chart" --name-template "$chart" --namespace zone "$@"
}

render_ha() {
    chart=$1
    shift
    render "$chart" -f "$charts/$chart/values-ha.yaml" "$@"
}

query() {
    printf '%s\n' "$1" | yq eval-all "$2" -
}

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    if [ $# -gt 1 ]; then
        printf '%s\n' "$2" >&2
    fi
    failures=$((failures + 1))
}

expect() {
    message=$1
    expected=$2
    rendered=$3
    expression=$4
    if ! actual=$(query "$rendered" "$expression" 2>&1); then
        fail "$message" " query failed: $expression
$actual"
        return
    fi
    if [ "$actual" != "$expected" ]; then
        fail "$message" " expected: $expected
      got: $actual
    query: $expression"
    fi
}

rendered_or_fail() {
    message=$1
    shift
    if ! rendered=$("$@" 2>&1); then
        fail "$message" " the render was refused:
$rendered"
        return 1
    fi
}

expect_refused() {
    message=$1
    reason=$2
    shift 2
    if refusal=$("$@" 2>&1 >/dev/null); then
        fail "$message" ' rendered instead of being refused'
        return
    fi
    case "$refusal" in
        *"$reason"*) ;;
        *) fail "$message" " refused without \"$reason\":
$refusal" ;;
    esac
}

expect_renders() {
    message=$1
    shift
    rendered_or_fail "$message" "$@" || true
}

apps_server='select(.kind == "Deployment" and .metadata.name == "zone-apps-server")'
apps_manager='select(.kind == "Deployment" and .metadata.name == "zone-apps-manager")'
apps_container="$apps_server | .spec.template.spec.containers[] | select(.name == \"server\")"
apps_egress='select(.kind == "NetworkPolicy" and .metadata.name == "zone-apps-server-egress") | .spec.egress[]'
apps_selectors='select(.kind == "Deployment") | .spec.template.spec | ((.topologySpreadConstraints[]?.labelSelector), (.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[]?.podAffinityTerm.labelSelector))'
apps_spread_policy='(.maxSkew | tostring) + " " + .whenUnsatisfiable + " " + .nodeTaintsPolicy + " " + ((.matchLabelKeys // []) | join(","))'
apps_labels='.matchLabels | to_entries | map(.key + "=" + .value) | join(",")'
apps_runner_names='[.. | select(tag == "!!map" and .name == "ZONE_RUNNER_NAME")] | map(.valueFrom.fieldRef.fieldPath // .value) | join(",")'

release_selectors() {
    printf '%s,%s,%s,%s,%s' "$1" "$1" "$1" "$1" "$1"
}

render_apps_ha() {
    render_ha zone-apps \
        --set "server.agentState.persistence.storageClass=$storage_class" \
        --set "server.trustedProxies=$trusted_proxies" \
        --set highAvailability.externalNetworkPolicy=true \
        "$@"
}

render_apps_ha_vouched() {
    render_apps_ha --set highAvailability.externalValkey=true "$@"
}

render_apps_ha_created() {
    render_apps_ha \
        --set secrets.create=true \
        --set secrets.dbPassword=test-password \
        --set "secrets.jwtSecret=$jwt_secret" \
        --set "secrets.encryptionKey=$encryption_key" \
        "$@"
}

mount_subpath() {
    printf '%s | .volumeMounts[] | select(.mountPath == "%s") | .name + ":" + .subPath' "$apps_container" "$1"
}

expect_shutdown_budget() {
    rendered=$1
    termination=$(query "$rendered" "$apps_server | .spec.template.spec.terminationGracePeriodSeconds")
    delay=$(query "$rendered" "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_DELAY_SECONDS\") | .value")
    grace=$(query "$rendered" "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_GRACE_SECONDS\") | .value")
    budget=$((delay + grace + shutdown_tail))
    if [ "$termination" -le "$budget" ]; then
        fail 'a terminating server has time for its whole shutdown' " terminationGracePeriodSeconds $termination is not over delay $delay + grace $grace + $shutdown_tail = $budget"
    fi
}

expect_shutdown_delay_outlasts_readiness() {
    rendered=$1
    delay=$(query "$rendered" "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_DELAY_SECONDS\") | .value")
    period=$(query "$rendered" "$apps_container | .readinessProbe.periodSeconds")
    threshold=$(query "$rendered" "$apps_container | .readinessProbe.failureThreshold")
    window=$((period * threshold + ingress_sync_seconds))
    if [ "$delay" -lt "$window" ]; then
        fail 'a draining server keeps serving until readiness has failed and the ingress has dropped it' " ZONE_SHUTDOWN_DELAY_SECONDS $delay is under periodSeconds $period x failureThreshold $threshold + $ingress_sync_seconds = $window"
    fi
}

check_apps_ha() {
    rendered_or_fail 'the zone-apps HA profile renders' \
        render_apps_ha_vouched \
        --set-string commonEnv.REDIS_URL=redis://plain.example.com:6379 \
        --set-string server.env.REDIS_URL=redis://plain.example.com:6379 || return 0

    expect 'the server autoscales from 3 replicas' 3 "$rendered" \
        'select(.kind == "HorizontalPodAutoscaler" and .metadata.name == "zone-apps-server") | .spec.minReplicas'
    expect 'the server autoscales to 10 replicas' 10 "$rendered" \
        'select(.kind == "HorizontalPodAutoscaler" and .metadata.name == "zone-apps-server") | .spec.maxReplicas'
    expect 'the server rolls one surge pod at a time' 'RollingUpdate 1 0' "$rendered" \
        "$apps_server | .spec.strategy | .type + \" \" + (.rollingUpdate.maxSurge | tostring) + \" \" + (.rollingUpdate.maxUnavailable | tostring)"
    expect 'the server PDB allows one disruption and sets no minAvailable' '1 false' "$rendered" \
        'select(.kind == "PodDisruptionBudget" and .metadata.name == "zone-apps-server") | (.spec.maxUnavailable | tostring) + " " + (.spec | has("minAvailable") | tostring)'
    expect 'a drain may evict a server pod that is not Ready' AlwaysAllow "$rendered" \
        'select(.kind == "PodDisruptionBudget" and .metadata.name == "zone-apps-server") | .spec.unhealthyPodEvictionPolicy'

    expect 'the server prefers spreading across hostnames' kubernetes.io/hostname "$rendered" \
        "$apps_server | .spec.template.spec.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.topologyKey"
    expect 'the server spreads each rollout by hostname, refusing to schedule past a skew of 1' '1 DoNotSchedule Honor pod-template-hash' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"kubernetes.io/hostname\") | $apps_spread_policy"
    expect 'the server spreads each rollout by zone on a best-effort basis' '1 ScheduleAnyway Honor pod-template-hash' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"topology.kubernetes.io/zone\") | $apps_spread_policy"
    expect 'the console spreads each rollout by hostname on a best-effort basis' '1 ScheduleAnyway Honor pod-template-hash' "$rendered" \
        "$apps_manager | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"kubernetes.io/hostname\") | $apps_spread_policy"
    expect 'every topology spread constraint ignores tainted and cordoned nodes' 0 "$rendered" \
        '[select(.kind == "Deployment") | .spec.template.spec.topologySpreadConstraints[] | select(.nodeTaintsPolicy != "Honor")] | length'
    expect 'every topology spread constraint counts only its own rollout revision' '3 0' "$rendered" \
        '[select(.kind == "Deployment") | .spec.template.spec.topologySpreadConstraints[]] | (length | tostring) + " " + ([.[] | select((.matchLabelKeys // []) | contains(["pod-template-hash"]) | not)] | length | tostring)'
    expect 'every spread and anti-affinity selector counts only this release' "$(release_selectors zone-apps)" "$rendered" \
        "[$apps_selectors | .matchLabels[\"app.kubernetes.io/instance\"]] | join(\",\")"
    expect 'every spread and anti-affinity selector keeps its component' manager,manager,server,server,server "$rendered" \
        "[$apps_selectors | .matchLabels[\"app.kubernetes.io/component\"]] | join(\",\")"
    expect 'a surge pod with maxUnavailable 0 never waits on required anti-affinity' 0 "$rendered" \
        '[select(.kind == "Deployment" and .spec.strategy.rollingUpdate.maxUnavailable == 0) | .spec.template.spec.affinity.podAntiAffinity.requiredDuringSchedulingIgnoredDuringExecution[]?] | length'
    expect 'neither the server nor the console requires anti-affinity' 0 "$rendered" \
        '[select(.kind == "Deployment") | .spec.template.spec.affinity.podAntiAffinity.requiredDuringSchedulingIgnoredDuringExecution[]?] | length'

    expect 'readiness reads /health/ready every 5 seconds' '/health/ready 5 3 2' "$rendered" \
        "$apps_container | .readinessProbe | .httpGet.path + \" \" + (.periodSeconds | tostring) + \" \" + (.timeoutSeconds | tostring) + \" \" + (.failureThreshold | tostring)"
    expect 'liveness reads /health every 10 seconds' '/health 10 3' "$rendered" \
        "$apps_container | .livenessProbe | .httpGet.path + \" \" + (.periodSeconds | tostring) + \" \" + (.failureThreshold | tostring)"
    expect 'the startup probe gives migrations 10 minutes' '/health 10 60' "$rendered" \
        "$apps_container | .startupProbe | .httpGet.path + \" \" + (.periodSeconds | tostring) + \" \" + (.failureThreshold | tostring)"
    expect 'a terminating server has 75 seconds to drain' 75 "$rendered" \
        "$apps_server | .spec.template.spec.terminationGracePeriodSeconds"
    expect 'the server keeps serving 15 seconds after SIGTERM' 15 "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_DELAY_SECONDS\") | .value"
    expect 'the server gives HTTP 30 seconds to finish' 30 "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_GRACE_SECONDS\") | .value"
    expect_shutdown_budget "$rendered"
    expect_shutdown_delay_outlasts_readiness "$rendered"

    expect 'the shared claim is ReadWriteMany on the given storage class' "ReadWriteMany $storage_class" "$rendered" \
        'select(.kind == "PersistentVolumeClaim" and .metadata.name == "zone-apps-shared-state") | .spec.accessModes[0] + " " + .spec.storageClassName'
    expect 'the server mounts the shared claim' zone-apps-shared-state "$rendered" \
        "$apps_server | .spec.template.spec.volumes[] | select(.name == \"agent-state\") | .persistentVolumeClaim.claimName"
    expect 'agent state is a subPath of the shared claim' agent-state:agent-state "$rendered" "$(mount_subpath /app/agent-state)"
    expect 'clones are a subPath of the shared claim' agent-state:clones "$rendered" "$(mount_subpath /app/clones)"
    expect 'artifacts are a subPath of the shared claim' agent-state:artifacts "$rendered" "$(mount_subpath /app/artifacts)"
    expect 'the chat workspace is a subPath of the shared claim' agent-state:workspace "$rendered" "$(mount_subpath /app/workspace)"

    expect 'the console autoscales from 2 replicas' 2 "$rendered" \
        'select(.kind == "HorizontalPodAutoscaler" and .metadata.name == "zone-apps-manager") | .spec.minReplicas'
    expect 'the console PDB allows one disruption and sets no minAvailable' '1 false' "$rendered" \
        'select(.kind == "PodDisruptionBudget" and .metadata.name == "zone-apps-manager") | (.spec.maxUnavailable | tostring) + " " + (.spec | has("minAvailable") | tostring)'
    expect 'a drain may evict a console pod that is not Ready' AlwaysAllow "$rendered" \
        'select(.kind == "PodDisruptionBudget" and .metadata.name == "zone-apps-manager") | .spec.unhealthyPodEvictionPolicy'
    expect 'the console prefers spreading across hostnames' kubernetes.io/hostname "$rendered" \
        "$apps_manager | .spec.template.spec.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.topologyKey"

    expect 'the ingress keeps a browser on one server replica' zone-server "$rendered" \
        'select(.kind == "Service" and .metadata.name == "zone-apps-server") | .metadata.annotations["haproxy.org/cookie-persistence"]'

    expect 'REDIS_URL is rendered once, from the secret' valkey-url "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL")] | map(.valueFrom.secretKeyRef.key // "plain") | join(",")'
    expect 'no env var named REDIS_URL carries a plain value' 0 "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL" and has("value"))] | length'

    expect 'the server trusts the given proxies' "$trusted_proxies" "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_TRUSTED_PROXIES\") | .value"
    expect 'each server pod registers as a runner named after itself, once' metadata.name "$rendered" "$apps_runner_names"
}

expect_shutdown_check_catches() {
    setting=$1
    check=$2
    shift 2
    rendered_or_fail "the zone-apps HA profile renders with $setting" render_apps_ha_vouched "$@" || return 0
    before=$failures
    "$check" "$rendered" 2>/dev/null
    caught=$((failures - before))
    failures=$before
    if [ "$caught" -eq 0 ]; then
        fail "the shutdown checks catch $setting"
    fi
}

check_apps_ha_shutdown_checks() {
    expect_shutdown_check_catches 'a termination grace that only equals delay + grace + 17' expect_shutdown_budget \
        --set server.terminationGracePeriodSeconds=62
    expect_shutdown_check_catches 'a delay that ends when readiness has failed, before the ingress syncs' \
        expect_shutdown_delay_outlasts_readiness --set-string server.env.ZONE_SHUTDOWN_DELAY_SECONDS=10
}

check_apps_ha_trusted_proxies() {
    rendered_or_fail 'the zone-apps HA profile renders trusting loopback only' \
        render_apps_ha_vouched --set-string 'server.trustedProxies=127.0.0.1/32\,::1/128' || return 0
    expect 'the explicit loopback opt-out reaches the server' "$loopback" "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_TRUSTED_PROXIES\") | .value"

    expect_renders 'server.env.ZONE_TRUSTED_PROXIES satisfies the HA profile on its own' \
        render_apps_ha_vouched --set server.trustedProxies= --set server.env.ZONE_TRUSTED_PROXIES=10.42.0.0/16
}

check_apps_ha_runner_name() {
    rendered_or_fail 'the zone-apps HA profile renders with a blank server.env.ZONE_RUNNER_NAME' \
        render_apps_ha_vouched --set-string server.env.ZONE_RUNNER_NAME= --set-string commonEnv.ZONE_RUNNER_NAME= || return 0
    expect 'a blank ZONE_RUNNER_NAME leaves the pod name as the only runner name' metadata.name "$rendered" "$apps_runner_names"
}

check_apps_ha_release() {
    rendered_or_fail 'the zone-apps HA profile renders as another release' \
        render_apps_ha_vouched --name-template other || return 0
    expect 'every spread and anti-affinity selector names its own release' "$(release_selectors other)" "$rendered" \
        "[$apps_selectors | .matchLabels[\"app.kubernetes.io/instance\"]] | join(\",\")"
}

check_apps_ha_selector_override() {
    rendered_or_fail 'the zone-apps HA profile renders with custom server spread selectors' \
        render_apps_ha_vouched --set-json 'server.topologySpreadConstraints=[
            {"maxSkew": 1, "topologyKey": "kubernetes.io/hostname", "whenUnsatisfiable": "DoNotSchedule", "matchLabelKeys": ["pod-template-hash"],
             "labelSelector": {"matchLabels": {"app.kubernetes.io/component": "server", "tier": "api"}}},
            {"maxSkew": 1, "topologyKey": "topology.kubernetes.io/zone", "whenUnsatisfiable": "ScheduleAnyway",
             "labelSelector": {"matchExpressions": [{"key": "tier", "operator": "In", "values": ["api"]}]}}
        ]' || return 0
    expect 'a custom spread selector keeps its labels and gains the release' \
        app.kubernetes.io/component=server,app.kubernetes.io/instance=zone-apps,tier=api "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"kubernetes.io/hostname\") | .labelSelector | $apps_labels"
    expect 'a spread selector of only expressions keeps them and gains the release' \
        'tier In api|app.kubernetes.io/instance=zone-apps' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"topology.kubernetes.io/zone\") | .labelSelector | (.matchExpressions | map(.key + \" \" + .operator + \" \" + (.values | join(\",\"))) | join(\",\")) + \"|\" + ($apps_labels)"
}

check_apps_ha_secret() {
    url='rediss://zone:p%40ss@redis.example.com:6380/0'
    rendered_or_fail 'secrets.create with a rediss:// URL renders' \
        render_apps_ha_created --set-string "secrets.redisUrl=$url" || return 0
    expect 'secrets.redisUrl is the secret valkey-url' "$url" "$rendered" \
        'select(.kind == "Secret") | .stringData["valkey-url"]'

    expect_renders 'a redis:// URL on a host that only starts with valkey renders' \
        render_apps_ha_created --set-string secrets.redisUrl=redis://valkey-cache.example.com:6379
    expect_renders 'a redis:// URL on a host under a valkey domain outside the cluster renders' \
        render_apps_ha_created --set-string secrets.redisUrl=redis://cache.valkey.example.com:6379
    expect_renders 'a redis:// URL on an IP address renders' \
        render_apps_ha_created --set-string 'secrets.redisUrl=redis://10.0.0.5:6379'
    for url in redis://valkey.corp:6379 redis://valkey.example.com:6379 rediss://valkey.svc.example.com:6380; do
        expect_renders "a secrets.redisUrl on an external host named valkey ($url) renders" \
            render_apps_ha_created --set-string "secrets.redisUrl=$url"
    done
}

check_apps_ha_refusals() {
    expect_refused 'HA without an external Valkey is refused' 'needs an external Redis or Valkey' \
        render_apps_ha
    expect_refused 'HA with secrets.create and no secrets.redisUrl is refused' 'set secrets.redisUrl' \
        render_apps_ha_created
    for url in \
        redis://valkey:6379 \
        redis://valkey \
        redis://VALKEY:6379 \
        redis://valkey.zone:6379 \
        redis://valkey.zone.svc:6379 \
        redis://valkey.zone.svc.cluster.local:6379 \
        rediss://:secret@valkey.zone.svc.cluster.local.:6380 \
        redis://10-244-1-5.valkey.zone.svc.cluster.local:6379 \
        redis://valkey-0.valkey.zone.svc:6379 \
        redis://valkey.other.svc:6379 \
        redis://valkey.other.svc.cluster.local:6379; do
        expect_refused "secrets.redisUrl naming the in-cluster valkey ($url) is refused" 'in-cluster valkey service' \
            render_apps_ha_created --set-string "secrets.redisUrl=$url"
    done
    expect_refused 'secrets.redisUrl naming the valkey service of the release namespace is refused' 'in-cluster valkey service (valkey.other)' \
        render_apps_ha_created --namespace other --set-string secrets.redisUrl=redis://valkey.other:6379
    expect_refused 'the schema refuses a secrets.redisUrl scheme other than redis or rediss' "/secrets/redisUrl" \
        render_apps_ha_created --set-string secrets.redisUrl=http://redis.example.com:6379
    expect_refused 'the template refuses a secrets.redisUrl scheme other than redis or rediss' 'must start with redis:// or rediss://' \
        render_apps_ha_created --skip-schema-validation --set-string secrets.redisUrl=http://redis.example.com:6379
    expect_refused 'HA with ReadWriteOnce agent state is refused' 'needs agent state shared by every replica' \
        render_apps_ha_vouched --set server.agentState.persistence.accessMode=ReadWriteOnce
    expect_refused 'HA without agent state persistence is refused' 'needs agent state shared by every replica' \
        render_apps_ha_vouched --set server.agentState.persistence.enabled=false
    for reason in \
        'devices: enforce a NetworkPolicy of your own that admits only the ingress controller to the server and set highAvailability.externalNetworkPolicy=true' \
        'cuts the server off from single sign-on identity providers, SMTP, git hosts and model providers'; do
        expect_refused "HA trusting a pod network without a network policy is refused: $reason" "$reason" \
            render_apps_ha_vouched --set highAvailability.externalNetworkPolicy=false
    done
    expect_refused 'a ReadWriteMany claim with no storage class is refused' 'storageClass must name a ReadWriteMany storage class' \
        render_ha zone-apps --set highAvailability.externalValkey=true --set "server.trustedProxies=$trusted_proxies" \
            --set highAvailability.externalNetworkPolicy=true
    for reason in server.trustedProxies "$trusted_proxies" "$loopback"; do
        expect_refused "HA without trusted proxies is refused, naming $reason" "$reason" \
            render_apps_ha_vouched --set server.trustedProxies=
    done
    expect_refused 'HA with a blank server.trustedProxies is refused' server.trustedProxies \
        render_apps_ha_vouched --set-string 'server.trustedProxies= '
    expect_refused 'HA with one ZONE_RUNNER_NAME in server.env is refused' server.env.ZONE_RUNNER_NAME \
        render_apps_ha_vouched --set server.env.ZONE_RUNNER_NAME=zone
    expect_refused 'HA with one ZONE_RUNNER_NAME in commonEnv is refused' commonEnv.ZONE_RUNNER_NAME \
        render_apps_ha_vouched --set commonEnv.ZONE_RUNNER_NAME=zone
    expect_refused 'HA network policy without the external Valkey CIDR is refused' 'networkPolicy.externalValkeyCIDR is required' \
        render_apps_ha_vouched --set networkPolicy.enabled=true --set networkPolicy.externalValkeyPort=6379
    expect_refused 'HA network policy without the external Valkey port is refused' 'networkPolicy.externalValkeyPort is required' \
        render_apps_ha_vouched --set networkPolicy.enabled=true --set networkPolicy.externalValkeyCIDR=10.20.0.0/16
}

check_apps_ha_network_policy() {
    rendered_or_fail 'HA network policy with the external Valkey CIDR and port renders' \
        render_apps_ha_vouched \
        --set networkPolicy.enabled=true \
        --set networkPolicy.externalValkeyCIDR=10.20.0.0/16 \
        --set networkPolicy.externalValkeyPort=6380 || return 0
    expect 'the server may reach the external Valkey CIDR on its port' '10.20.0.0/16 6380' "$rendered" \
        "$apps_egress | select(.to[0].ipBlock.cidr == \"10.20.0.0/16\") | .to[0].ipBlock.cidr + \" \" + (.ports[0].port | tostring)"
    expect 'the server is not sent to in-cluster valkey pods' 0 "$rendered" \
        "[$apps_egress | select(.to[0].podSelector.matchLabels[\"app.kubernetes.io/name\"] == \"valkey\")] | length"
}

check_apps_ha_operator_values() {
    operator=$scratch/zone-apps-operator.yaml
    cat >"$operator" <<EOF
highAvailability:
  externalValkey: true
  externalNetworkPolicy: true
server:
  trustedProxies: $trusted_proxies
  agentState:
    persistence:
      storageClass: $storage_class
EOF
    rendered_or_fail 'the zone-apps HA profile layered over an operator values file renders' \
        render zone-apps -f "$operator" -f "$charts/zone-apps/values-ha.yaml" || return 0
    expect "the operator file's storage class reaches the shared claim" "ReadWriteMany $storage_class" "$rendered" \
        'select(.kind == "PersistentVolumeClaim" and .metadata.name == "zone-apps-shared-state") | .spec.accessModes[0] + " " + .spec.storageClassName'
    expect "the operator file's trusted proxies reach the server" "$trusted_proxies" "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_TRUSTED_PROXIES\") | .value"
}

check_apps_ha_shared_claim() {
    rendered_or_fail 'the shared claim renders on its own, as docs/HA.md creates it before a move' \
        render_apps_ha_vouched --show-only templates/server/pvc.yaml || return 0
    expect 'the shared claim renders alone, under the name and owner docs/HA.md gives' \
        'PersistentVolumeClaim zone-apps-shared-state Helm' "$rendered" \
        '.kind + " " + .metadata.name + " " + .metadata.labels["app.kubernetes.io/managed-by"]'
}

check_apps_default() {
    rendered_or_fail 'the zone-apps defaults render' render zone-apps || return 0
    expect 'the default server has no startup probe' null "$rendered" "$apps_container | .startupProbe"
    expect 'the default server is ready on /health' /health "$rendered" "$apps_container | .readinessProbe.httpGet.path"
    expect 'the default render has no ReadWriteMany claim' 0 "$rendered" \
        '[select(.kind == "PersistentVolumeClaim") | .spec.accessModes[] | select(. == "ReadWriteMany")] | length'
    expect 'the default spread and anti-affinity selectors name no release' 0 "$rendered" \
        "[$apps_selectors | select(.matchLabels | has(\"app.kubernetes.io/instance\"))] | length"
    expect 'the default spread constraints count every rollout revision' 0 "$rendered" \
        '[select(.kind == "Deployment") | .spec.template.spec.topologySpreadConstraints[] | select(has("matchLabelKeys"))] | length'
    expect 'the default server keeps its own termination grace' null "$rendered" \
        "$apps_server | .spec.template.spec.terminationGracePeriodSeconds"
    expect 'the default PDBs keep the Kubernetes unhealthy pod eviction policy' 0 "$rendered" \
        '[select(.kind == "PodDisruptionBudget") | select(.spec | has("unhealthyPodEvictionPolicy"))] | length'
    expect 'the default render sets no runner name' '' "$rendered" "$apps_runner_names"

    rendered_or_fail 'the zone-apps defaults render with network policies' render zone-apps --set networkPolicy.enabled=true || return 0
    expect 'the default server egress reaches the in-cluster valkey pods' 1 "$rendered" \
        "[$apps_egress | select(.to[0].podSelector.matchLabels[\"app.kubernetes.io/name\"] == \"valkey\")] | length"

    expect_refused 'two fixed replicas sharing server.env.ZONE_RUNNER_NAME are refused' server.env.ZONE_RUNNER_NAME \
        render zone-apps --set server.autoscaling.enabled=false --set server.replicaCount=2 --set server.env.ZONE_RUNNER_NAME=zone
    expect_refused 'autoscaled replicas sharing commonEnv.ZONE_RUNNER_NAME are refused' commonEnv.ZONE_RUNNER_NAME \
        render zone-apps --set server.autoscaling.enabled=true --set commonEnv.ZONE_RUNNER_NAME=zone
    rendered_or_fail 'one replica with its own ZONE_RUNNER_NAME renders' \
        render zone-apps --set server.autoscaling.enabled=false --set server.replicaCount=1 --set server.env.ZONE_RUNNER_NAME=zone || return 0
    expect 'one replica keeps its server.env.ZONE_RUNNER_NAME' zone "$rendered" "$apps_runner_names"

    rendered_or_fail 'a plain commonEnv.REDIS_URL renders' \
        render zone-apps --set-string commonEnv.REDIS_URL=redis://plain.example.com:6379 || return 0
    expect 'commonEnv.REDIS_URL is never a plain env var' 0 "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL" and has("value"))] | length'
}

infra_cluster='select(.kind == "Cluster" and .metadata.name == "zone-postgres")'
infra_backup='select(.kind == "ScheduledBackup" and .metadata.name == "zone-postgres")'
infra_valkey='[select(.metadata.name == "valkey" or .metadata.name == "valkey-pvc") | .kind] | join(",")'
barman_cloud_plugin=barman-cloud.cloudnative-pg.io
infra_bootstrap="$infra_cluster | .spec.bootstrap | keys | join(\",\")"
infra_external="$infra_cluster | [.spec.externalClusters[]? | .name + \" \" + .plugin.name + \" \" + .plugin.parameters.barmanObjectName + \" \" + .plugin.parameters.serverName] | join(\",\")"
infra_recovery="$infra_cluster | .spec.bootstrap.recovery | .source + \" \" + .database + \" \" + .owner + \" \" + .secret.name"
infra_archive="$infra_cluster | [.spec.plugins[]? | .name + \" \" + (.isWALArchiver | tostring) + \" \" + .parameters.barmanObjectName] | join(\",\")"
recovery_store=zone-backups
recovery_server=zone-postgres
recovery_time=2026-11-20T09:30:00Z
restored_store=zone-backups-restored

expect_initdb() {
    expect "$1 bootstraps with initdb alone" initdb "$2" "$infra_bootstrap"
    expect "$1 names no external cluster" '' "$2" "$infra_external"
}

expect_recovery() {
    expect "$1 bootstraps with recovery alone" recovery "$2" "$infra_bootstrap"
    expect "$1 recovers the chart's database, owner and secret from the origin external cluster" \
        'origin litellm zone zone-secrets' "$2" "$infra_recovery"
    expect "$1 reads the old backups through the Barman Cloud plugin" \
        "origin $barman_cloud_plugin $recovery_store $recovery_server" "$2" "$infra_external"
    expect "$1 archives WAL into the new object store" \
        "$barman_cloud_plugin true $restored_store" "$2" "$infra_archive"
}

ai_litellm='select(.kind == "Deployment" and .metadata.name == "litellm")'
ai_ollama='select(.kind == "Deployment" and .metadata.name == "ollama")'
ai_pdb='select(.kind == "PodDisruptionBudget" and .metadata.name == "litellm")'
ai_litellm_init="$ai_litellm | [.spec.template.spec.initContainers[]?.name] | join(\",\")"
ai_litellm_workers="$ai_litellm | .spec.template.spec.containers[0].args as \$arguments | \$arguments | to_entries | .[] | select(.value == \"--num_workers\") | \$arguments[.key + 1]"
litellm_process_mebibytes=1024

mebibytes() {
    case $1 in
        *Gi) number=${1%Gi} multiplier=1024 ;;
        *Mi) number=${1%Mi} multiplier=1 ;;
        *) number=$1 multiplier='' ;;
    esac
    case "$number$multiplier" in
        *[!0-9]* | '') printf '%s' "$1" ;;
        *) printf '%s' $((number * multiplier)) ;;
    esac
}

expect_at_most() {
    message=$1
    limit=$2
    value=$3
    case "$limit$value" in
        *[!0-9]* | '')
            fail "$message" " not numbers: limit \"$limit\", value \"$value\""
            return
            ;;
    esac
    if [ "$value" -gt "$limit" ]; then
        fail "$message" " $value exceeds $limit"
    fi
}

check_infra_default() {
    rendered_or_fail 'the zone-infra defaults render' render zone-infra || return 0
    expect 'the default cluster runs one instance' 1 "$rendered" "$infra_cluster | .spec.instances"
    expect 'the default cluster sets no postgresql block' false "$rendered" "$infra_cluster | .spec | has(\"postgresql\")"
    expect 'the default cluster leaves affinity to CNPG' false "$rendered" "$infra_cluster | .spec | has(\"affinity\")"
    expect 'the default cluster loads no plugins' false "$rendered" "$infra_cluster | .spec | has(\"plugins\")"
    expect 'the default render schedules no backup' 0 "$rendered" "[$infra_backup] | length"
    expect 'the default render deploys the in-cluster valkey' PersistentVolumeClaim,Service,Deployment "$rendered" "$infra_valkey"
    expect_initdb 'the default cluster' "$rendered"
}

check_infra_ha() {
    rendered_or_fail 'the zone-infra HA profile renders' render_ha zone-infra || return 0
    expect 'the HA cluster runs three instances' 3 "$rendered" "$infra_cluster | .spec.instances"
    expect 'a commit waits for any one standby, and blocks without one' 'any 1 required' "$rendered" \
        "$infra_cluster | .spec.postgresql.synchronous | .method + \" \" + (.number | tostring) + \" \" + .dataDurability"
    expect 'every instance requires a node of its own' 'true required kubernetes.io/hostname' "$rendered" \
        "$infra_cluster | .spec.affinity | (.enablePodAntiAffinity | tostring) + \" \" + .podAntiAffinityType + \" \" + .topologyKey"
    expect 'the HA cluster loads no plugins without an object store' false "$rendered" "$infra_cluster | .spec | has(\"plugins\")"
    expect 'the HA render schedules no backup without an object store' 0 "$rendered" "[$infra_backup] | length"
    expect 'the HA render deploys no in-cluster valkey' '' "$rendered" "$infra_valkey"
    expect_initdb 'the HA cluster' "$rendered"
    expect 'the HA render has no Valkey Deployment' 0 "$rendered" \
        '[select(.kind == "Deployment" and .spec.template.metadata.labels["app.kubernetes.io/name"] == "valkey")] | length'

    infra=$rendered
    rendered_or_fail 'the zone-apps HA profile renders for sizing' render_apps_ha_vouched || return 0
    apps=$rendered
    rendered_or_fail 'the zone-ai HA profile renders for sizing' render_ha zone-ai || return 0
    expect_connection_budget 'max_connections covers every server and LiteLLM pool at the HPA maximum plus a surge pod' \
        "$infra" "$apps" "$rendered"
}

expect_connection_budget() {
    message=$1
    max_connections=$(query "$2" "$infra_cluster | .spec.postgresql.parameters.max_connections")
    server_replicas=$(query "$3" 'select(.kind == "HorizontalPodAutoscaler" and .metadata.name == "zone-apps-server") | .spec.maxReplicas')
    litellm_replicas=$(query "$4" "$ai_litellm | .spec.replicas")
    litellm_workers=$(query "$4" "$ai_litellm_workers")
    case "$server_replicas$litellm_replicas$litellm_workers" in
        *[!0-9]* | '')
            fail "$message" " sizing inputs are not numbers: server $server_replicas, litellm $litellm_replicas x $litellm_workers"
            return
            ;;
    esac
    needed=$(((server_replicas + 1) * (20 + 1) + (litellm_replicas + 1) * litellm_workers * 10))
    expect_at_most "$message" "$max_connections" "$needed"
}

check_infra_ha_sizing() {
    rendered_or_fail 'the zone-apps HA profile renders for sizing' render_apps_ha_vouched || return 0
    apps=$rendered
    connections=$scratch/zone-infra-sizing.yaml
    printf '%s\n' 'postgres:' '  parameters:' '    max_connections: "500"' >"$connections"
    workers=$scratch/zone-ai-sizing.yaml
    printf '%s\n' 'litellm:' '  workers: 6' >"$workers"

    rendered_or_fail 'the zone-infra HA profile renders over a values file that sets max_connections' \
        render zone-infra -f "$connections" -f "$charts/zone-infra/values-ha.yaml" || return 0
    expect 'the profile overrides a max_connections set in your values file, as docs/HA.md warns' 400 "$rendered" \
        "$infra_cluster | .spec.postgresql.parameters.max_connections"
    infra=$rendered
    rendered_or_fail 'the zone-ai HA profile renders over a values file that sets litellm.workers' \
        render zone-ai -f "$workers" -f "$charts/zone-ai/values-ha.yaml" || return 0
    before=$failures
    expect_connection_budget 'over budget' "$infra" "$apps" "$rendered" 2>/dev/null
    caught=$((failures - before))
    failures=$before
    if [ "$caught" -eq 0 ]; then
        fail 'the budget check catches 6 LiteLLM workers in your values file against the 400 the profile keeps'
    fi

    rendered_or_fail 'the zone-infra HA profile renders with max_connections in a values file after it' \
        render_ha zone-infra -f "$connections" || return 0
    expect 'a max_connections in a values file after the profile wins' 500 "$rendered" \
        "$infra_cluster | .spec.postgresql.parameters.max_connections"

    rendered_or_fail 'the zone-infra HA profile renders with max_connections set after it' \
        render_ha zone-infra --set postgres.parameters.max_connections=500 || return 0
    infra=$rendered
    rendered_or_fail 'the zone-apps HA profile renders with maxReplicas set after it' \
        render_apps_ha_vouched --set server.autoscaling.maxReplicas=14 || return 0
    apps=$rendered
    rendered_or_fail 'the zone-ai HA profile renders for sizing' render_ha zone-ai || return 0
    expect_connection_budget 'the max_connections docs/HA.md gives for 14 server replicas covers their pools' \
        "$infra" "$apps" "$rendered"
}

check_infra_ha_parameters() {
    rendered_or_fail 'the zone-infra HA profile with a numeric parameter renders' \
        render_ha zone-infra --set postgres.parameters.max_connections=500 || return 0
    expect 'a postgresql parameter set as a number renders as a string, as CNPG requires' '!!str 500' "$rendered" \
        "$infra_cluster | .spec.postgresql.parameters.max_connections | tag + \" \" + ."
}

check_infra_ha_backup() {
    rendered_or_fail 'the zone-infra HA profile with an object store renders' \
        render_ha zone-infra --set postgres.backup.objectStore=zone-backups || return 0
    expect 'the cluster archives WAL through the Barman Cloud plugin' "$barman_cloud_plugin true zone-backups" "$rendered" \
        "$infra_cluster | .spec.plugins[] | .name + \" \" + (.isWALArchiver | tostring) + \" \" + .parameters.barmanObjectName"
    expect_initdb 'a cluster that archives WAL' "$rendered"
    expect 'the scheduled backup targets the cluster through the plugin' "zone-postgres plugin $barman_cloud_plugin" "$rendered" \
        "$infra_backup | .spec | .cluster.name + \" \" + .method + \" \" + .pluginConfiguration.name"
    expect 'the scheduled backup prefers a standby' prefer-standby "$rendered" "$infra_backup | .spec.target"
    expect 'the scheduled backup has a six-field schedule' 6 "$rendered" \
        "$infra_backup | .spec.schedule | split(\" \") | length"
}

check_infra_ha_operator_values() {
    operator=$scratch/zone-infra-operator.yaml
    printf '%s\n' 'postgres:' '  backup:' '    objectStore: zone-backups' >"$operator"
    rendered_or_fail 'the zone-infra HA profile layered over an operator values file renders' \
        render zone-infra -f "$operator" -f "$charts/zone-infra/values-ha.yaml" || return 0
    expect "the operator file's object store archives WAL" "$barman_cloud_plugin true zone-backups" "$rendered" \
        "$infra_cluster | .spec.plugins[] | .name + \" \" + (.isWALArchiver | tostring) + \" \" + .parameters.barmanObjectName"
    expect "the operator file's object store schedules the backup" "zone-postgres plugin $barman_cloud_plugin" "$rendered" \
        "$infra_backup | .spec | .cluster.name + \" \" + .method + \" \" + .pluginConfiguration.name"
}

check_infra_ha_refusals() {
    expect_refused 'synchronous replication with too few standbys is refused' 'must be below postgres.instances' \
        render_ha zone-infra --set postgres.instances=1
}

render_infra_recovery() {
    render_ha zone-infra \
        --set "postgres.backup.objectStore=$restored_store" \
        --set "postgres.recovery.objectStore=$recovery_store" \
        --set "postgres.recovery.serverName=$recovery_server" \
        "$@"
}

check_infra_ha_recovery() {
    rendered_or_fail 'the zone-infra HA profile renders a recovery to a point in time' \
        render_infra_recovery --set "postgres.recovery.targetTime=$recovery_time" || return 0
    expect_recovery 'a point-in-time recovery' "$rendered"
    expect 'a point-in-time recovery stops at the target time alone' "targetTime=$recovery_time" "$rendered" \
        "$infra_cluster | .spec.bootstrap.recovery.recoveryTarget | to_entries | map(.key + \"=\" + .value) | join(\",\")"
    expect 'a recovered cluster still schedules backups into the new object store' "zone-postgres plugin $barman_cloud_plugin" "$rendered" \
        "$infra_backup | .spec | .cluster.name + \" \" + .method + \" \" + .pluginConfiguration.name"

    rendered_or_fail 'the zone-infra HA profile renders a recovery to the end of the archive' render_infra_recovery || return 0
    expect_recovery 'a recovery to the end of the archive' "$rendered"
    expect 'a recovery to the end of the archive sets no recovery target' false "$rendered" \
        "$infra_cluster | .spec.bootstrap.recovery | has(\"recoveryTarget\")"

    rendered_or_fail 'the zone-infra HA profile renders a recovery that archives nowhere' \
        render_infra_recovery --set postgres.backup.objectStore= || return 0
    expect 'a recovery without a new object store bootstraps with recovery alone' recovery "$rendered" "$infra_bootstrap"
    expect 'a recovery without a new object store archives no WAL' '' "$rendered" "$infra_archive"

    rendered_or_fail 'the zone-infra defaults render a recovery' render zone-infra \
        --set "postgres.recovery.objectStore=$recovery_store" \
        --set "postgres.recovery.serverName=$recovery_server" || return 0
    expect 'a single-instance recovery bootstraps with recovery alone' recovery "$rendered" "$infra_bootstrap"
}

check_infra_recovery_refusals() {
    expect_refused 'a recovery from the object store the cluster archives into is refused' \
        'a recovered cluster archives into a new ObjectStore' \
        render_infra_recovery --set "postgres.backup.objectStore=$recovery_store"
    expect_refused 'a recovery without the backed-up server name is refused' \
        'postgres.recovery.serverName must name the cluster' \
        render_infra_recovery --set postgres.recovery.serverName=
    expect_refused 'a recovery target time without an object store is refused' \
        'need postgres.recovery.objectStore' \
        render_ha zone-infra --set "postgres.recovery.targetTime=$recovery_time"
    expect_refused 'a recovery server name without an object store is refused' \
        'need postgres.recovery.objectStore' \
        render_ha zone-infra --set "postgres.recovery.serverName=$recovery_server"
    expect_refused 'a misspelt recovery setting is refused rather than bootstrapping an empty database' \
        "additional properties 'objectstore' not allowed" \
        render_ha zone-infra --set "postgres.recovery.objectstore=$recovery_store"
    expect_refused 'a recovery server name that is not a string is refused' \
        'got number, want string' \
        render_infra_recovery --set postgres.recovery.serverName=1
}

check_ai_default() {
    rendered_or_fail 'the zone-ai defaults render' render zone-ai || return 0
    expect 'the default LiteLLM runs one replica' 1 "$rendered" "$ai_litellm | .spec.replicas"
    expect 'the default LiteLLM sets no affinity' null "$rendered" "$ai_litellm | .spec.template.spec.affinity"
    expect 'the default render has no LiteLLM PDB' 0 "$rendered" "[$ai_pdb] | length"
    expect 'the default LiteLLM waits for the bundled Ollama' wait-for-ollama "$rendered" "$ai_litellm_init"
}

check_ai_without_ollama() {
    rendered_or_fail 'the zone-ai defaults render without Ollama' render zone-ai --set ollama.enabled=false || return 0
    expect 'LiteLLM without Ollama waits for nothing' '' "$rendered" "$ai_litellm_init"
    expect 'the zone-ai defaults without Ollama render no Ollama' 0 "$rendered" "[$ai_ollama] | length"

    rendered_or_fail 'the zone-ai HA profile renders without Ollama even when asked to wait for it' \
        render_ha zone-ai --set ollama.enabled=false --set litellm.waitForOllama=true || return 0
    expect 'LiteLLM never waits for an Ollama the release does not run' '' "$rendered" "$ai_litellm_init"
}

check_ai_ha() {
    rendered_or_fail 'the zone-ai HA profile renders' render_ha zone-ai || return 0
    expect 'LiteLLM runs two replicas' 2 "$rendered" "$ai_litellm | .spec.replicas"
    expect 'LiteLLM prefers spreading across hostnames' 'kubernetes.io/hostname litellm' "$rendered" \
        "$ai_litellm | .spec.template.spec.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm | .topologyKey + \" \" + .labelSelector.matchLabels[\"app.kubernetes.io/name\"]"
    expect 'LiteLLM requires no anti-affinity' 0 "$rendered" \
        "[$ai_litellm | .spec.template.spec.affinity.podAntiAffinity.requiredDuringSchedulingIgnoredDuringExecution[]?] | length"
    expect 'the LiteLLM PDB allows one disruption and sets no minAvailable' '1 false' "$rendered" \
        "$ai_pdb | (.spec.maxUnavailable | tostring) + \" \" + (.spec | has(\"minAvailable\") | tostring)"
    expect 'the LiteLLM PDB selects the LiteLLM pods' litellm "$rendered" \
        "$ai_pdb | .spec.selector.matchLabels[\"app.kubernetes.io/name\"]"
    expect 'a drain may evict a LiteLLM pod that is not Ready' AlwaysAllow "$rendered" \
        "$ai_pdb | .spec.unhealthyPodEvictionPolicy"
    expect 'LiteLLM in the HA profile starts without waiting for Ollama' '' "$rendered" "$ai_litellm_init"

    litellm_workers=$(query "$rendered" "$ai_litellm_workers")
    litellm_memory=$(mebibytes "$(query "$rendered" "$ai_litellm | .spec.template.spec.containers[0].resources.limits.memory")")
    case "$litellm_workers" in
        *[!0-9]* | '') fail 'the LiteLLM worker count is a number' " got \"$litellm_workers\"" ;;
        *) expect_at_most 'the LiteLLM memory limit holds its master and every worker process' \
            "$litellm_memory" "$(((litellm_workers + 1) * litellm_process_mebibytes))" ;;
    esac

    ha_ollama=$(query "$rendered" "$ai_ollama")
    rendered_or_fail 'the zone-ai defaults render beside the HA profile' render zone-ai || return 0
    default_ollama=$(query "$rendered" "$ai_ollama")
    if [ "$default_ollama" != "$ha_ollama" ]; then
        fail 'the HA profile leaves Ollama unchanged'
    fi
}

documented_commands() {
    awk '
        /^[[:space:]]*```/ {
            fenced = !fenced
            command = ""
            next
        }
        fenced {
            line = $0
            sub(/^[[:space:]]+/, "", line)
            if (command == "" && line !~ /^helm (template|upgrade) /) {
                next
            }
            continuing = sub(/[[:space:]]*\\$/, "", line)
            command = command (command == "" ? "" : " ") line
            if (!continuing) {
                print command
                command = ""
            }
            next
        }
        {
            line = $0
            while (match(line, /`helm (template|upgrade) [^`]*`/)) {
                print substr(line, RSTART + 1, RLENGTH - 2)
                line = substr(line, RSTART + RLENGTH)
            }
        }
    ' "$1"
}

documented_chart() {
    for chart in zone-apps zone-ai zone-infra; do
        case "$1 " in
            *" helm/$chart "*)
                printf '%s' "$chart"
                return
                ;;
        esac
    done
}

documented_object_store=$restored_store
documented_postgres_size=50Gi
documented_postgres_class=gp3-documented
documented_server_memory=3Gi
documented_litellm_workers=5

write_documented_values() {
    cat >"$scratch/documented-zone-infra.yaml" <<EOF
postgres:
  storage:
    size: $documented_postgres_size
    storageClass: $documented_postgres_class
EOF
    cat >"$scratch/documented-zone-apps.yaml" <<EOF
server:
  resources:
    limits:
      memory: $documented_server_memory
EOF
    cat >"$scratch/documented-zone-ai.yaml" <<EOF
litellm:
  workers: $documented_litellm_workers
EOF
}

expect_documented_values() {
    command=$1
    chart=$2
    rendered=$3
    case $chart in
        zone-infra)
            if [ "$(query "$rendered" "[$infra_cluster] | length")" != 0 ]; then
                expect "your values' Postgres storage survives: $command" \
                    "$documented_postgres_size $documented_postgres_class" "$rendered" \
                    "$infra_cluster | .spec.storage.size + \" \" + .spec.storage.storageClass"
                case $command in
                    *" postgres.recovery.objectStore="*) expect_recovery "the documented restore: $command" "$rendered" ;;
                    *) expect_initdb "the documented command: $command" "$rendered" ;;
                esac
            fi
            ;;
        zone-apps)
            if [ "$(query "$rendered" "[$apps_server] | length")" != 0 ]; then
                expect "your values' server memory limit survives: $command" \
                    "$documented_server_memory" "$rendered" "$apps_container | .resources.limits.memory"
            fi
            ;;
        zone-ai)
            if [ "$(query "$rendered" "[$ai_litellm] | length")" != 0 ]; then
                expect "your values' LiteLLM workers survive: $command" \
                    "$documented_litellm_workers" "$rendered" "$ai_litellm_workers"
            fi
            ;;
    esac
}

check_documented_command() {
    command=${1%% | *}
    chart=$(documented_chart "$command")
    if [ -z "$chart" ]; then
        fail 'every helm command in docs/HA.md names its chart' " $command"
        return
    fi
    case $command in
        *"-f <your values> "*"-f helm/$chart/values-ha.yaml"*) ;;
        *) fail 'every helm command in docs/HA.md layers your values, then the profile' " $command" ;;
    esac
    case $command in
        *...*) return ;;
        *[\$\`\;\&\(\)]*)
            fail 'every helm command in docs/HA.md is plain words' " $command"
            return
            ;;
    esac
    operator="$scratch/documented-$chart.yaml"
    words=$(printf '%s\n' "$command" | sed \
        -e 's/^helm upgrade --install /helm template /' \
        -e 's/^helm upgrade /helm template /' \
        -e "s|<your values>|'$operator'|g" \
        -e "s|<ObjectStore name>|$documented_object_store|g" \
        -e "s|<new ObjectStore>|$documented_object_store|g" \
        -e "s|<old ObjectStore>|$recovery_store|g" \
        -e "s|<target time>|$recovery_time|g" \
        -e "s|<ReadWriteMany class>|$storage_class|g" \
        -e "s|<ingress controller pod CIDR>|$trusted_proxies|g" \
        -e "s| helm/zone-| '$charts'/zone-|g")
    case $words in
        *'<'*)
            fail 'every placeholder in a docs/HA.md helm command has a test value' " $command"
            return
            ;;
    esac
    eval "set -- $words"
    rendered_or_fail "the documented command renders: $command" "$@" || return 0
    expect_documented_values "$command" "$chart" "$rendered"
}

check_documented_commands() {
    write_documented_values
    commands=$scratch/documented-commands
    documented_commands "$guide" >"$commands"
    count=0
    while IFS= read -r line; do
        count=$((count + 1))
        check_documented_command "$line"
    done <"$commands"
    if [ "$count" -lt 5 ]; then
        fail 'docs/HA.md documents its helm commands' " found $count"
    fi
    if ! grep -q '^helm upgrade zone-infra .* --set postgres\.recovery\.objectStore=' "$commands"; then
        fail 'docs/HA.md restores Postgres with a helm upgrade that sets postgres.recovery.objectStore'
    fi
}

expect_misdocumented_restore_fails() {
    message=$1
    reason=$2
    shift 2
    guide=$scratch/HA.md
    sed "$@" "$root/docs/HA.md" >"$guide"
    if cmp -s "$guide" "$root/docs/HA.md"; then
        fail "$message: the edit changed nothing in docs/HA.md"
        return
    fi
    if output=$(ZONE_HA_GUIDE=$guide sh "$0" check_documented_commands 2>&1); then
        fail "$message passes the run"
        return
    fi
    case "$output" in
        *"$reason"*) ;;
        *) fail "$message fails without \"$reason\"" " $output" ;;
    esac
}

check_a_misdocumented_restore_fails_the_run() {
    expect_misdocumented_restore_fails 'a restore without your values' "your values' Postgres storage survives" \
        -e '/^[[:space:]]*helm upgrade zone-infra .*\\$/{' -e 'N' -e '/postgres\.recovery\.objectStore=/s/-f <your values> //' -e '}'
    expect_misdocumented_restore_fails 'a restore that bootstraps with initdb' \
        'restores Postgres with a helm upgrade that sets postgres.recovery.objectStore' \
        -e 's/postgres\.recovery\.objectStore=<old ObjectStore>/postgres.backup.schedule=@daily/'
    expect_misdocumented_restore_fails 'a restore from the object store it archives into' \
        'a recovered cluster archives into a new ObjectStore' \
        -e 's/postgres\.recovery\.objectStore=<old ObjectStore>/postgres.recovery.objectStore=<new ObjectStore>/'
}

check_a_refused_render_fails_the_run() {
    broken=$scratch/broken
    cp -R "$charts" "$broken"
    for chart in zone-apps zone-ai; do
        printf '%s\n' 'broken: [' >>"$broken/$chart/values-ha.yaml"
    done
    if ZONE_HELM_CHARTS=$broken sh "$0" check_apps_ha check_ai_ha >/dev/null 2>&1; then
        fail 'a profile that no longer renders fails the run'
    fi
}

if [ $# -eq 0 ]; then
    set -- \
        check_apps_default \
        check_apps_ha \
        check_apps_ha_release \
        check_apps_ha_selector_override \
        check_apps_ha_secret \
        check_apps_ha_shutdown_checks \
        check_apps_ha_trusted_proxies \
        check_apps_ha_runner_name \
        check_apps_ha_refusals \
        check_apps_ha_network_policy \
        check_apps_ha_operator_values \
        check_apps_ha_shared_claim \
        check_infra_default \
        check_infra_ha \
        check_infra_ha_parameters \
        check_infra_ha_sizing \
        check_infra_ha_backup \
        check_infra_ha_operator_values \
        check_infra_ha_refusals \
        check_infra_ha_recovery \
        check_infra_recovery_refusals \
        check_ai_default \
        check_ai_ha \
        check_ai_without_ollama \
        check_documented_commands \
        check_a_misdocumented_restore_fails_the_run \
        check_a_refused_render_fails_the_run
fi
for check in "$@"; do
    case $check in
        check_*) "$check" ;;
        *)
            printf 'not a check: %s\n' "$check" >&2
            exit 2
            ;;
    esac
done

if [ "$failures" -gt 0 ]; then
    printf '%s Helm HA check(s) failed\n' "$failures" >&2
    exit 1
fi
printf '%s\n' 'Helm HA checks passed'

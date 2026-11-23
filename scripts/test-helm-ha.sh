#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
storage_class=test-rwx
trusted_proxies=10.244.0.0/16
loopback='127.0.0.1/32,::1/128'
failures=0

jwt_secret=0123456789abcdef0123456789abcdef
encryption_key=fedcba9876543210fedcba9876543210

render() {
    chart=$1
    shift
    helm template "$root/helm/$chart" --name-template "$chart" --namespace zone "$@"
}

render_ha() {
    chart=$1
    shift
    render "$chart" -f "$root/helm/$chart/values-ha.yaml" "$@"
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
    if ! output=$("$@" 2>&1); then
        fail "$message" " the render was refused:
$output"
        return 1
    fi
    printf '%s\n' "$output"
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
    rendered_or_fail "$message" "$@" >/dev/null || true
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

check_apps_ha() {
    rendered=$(rendered_or_fail 'the zone-apps HA profile renders' \
        render_apps_ha_vouched \
        --set-string commonEnv.REDIS_URL=redis://plain.example.com:6379 \
        --set-string server.env.REDIS_URL=redis://plain.example.com:6379) || return 0

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
    expect 'a terminating server has 60 seconds to drain' 60 "$rendered" \
        "$apps_server | .spec.template.spec.terminationGracePeriodSeconds"
    expect 'the server keeps serving 10 seconds after SIGTERM' 10 "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_DELAY_SECONDS\") | .value"
    expect 'the server gives HTTP 30 seconds to finish' 30 "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_SHUTDOWN_GRACE_SECONDS\") | .value"

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

check_apps_ha_trusted_proxies() {
    rendered=$(rendered_or_fail 'the zone-apps HA profile renders trusting loopback only' \
        render_apps_ha_vouched --set-string 'server.trustedProxies=127.0.0.1/32\,::1/128') || return 0
    expect 'the explicit loopback opt-out reaches the server' "$loopback" "$rendered" \
        "$apps_container | .env[] | select(.name == \"ZONE_TRUSTED_PROXIES\") | .value"

    expect_renders 'server.env.ZONE_TRUSTED_PROXIES satisfies the HA profile on its own' \
        render_apps_ha_vouched --set server.trustedProxies= --set server.env.ZONE_TRUSTED_PROXIES=10.42.0.0/16
}

check_apps_ha_runner_name() {
    rendered=$(rendered_or_fail 'the zone-apps HA profile renders with a blank server.env.ZONE_RUNNER_NAME' \
        render_apps_ha_vouched --set-string server.env.ZONE_RUNNER_NAME= --set-string commonEnv.ZONE_RUNNER_NAME=) || return 0
    expect 'a blank ZONE_RUNNER_NAME leaves the pod name as the only runner name' metadata.name "$rendered" "$apps_runner_names"
}

check_apps_ha_release() {
    rendered=$(rendered_or_fail 'the zone-apps HA profile renders as another release' \
        render_apps_ha_vouched --name-template other) || return 0
    expect 'every spread and anti-affinity selector names its own release' "$(release_selectors other)" "$rendered" \
        "[$apps_selectors | .matchLabels[\"app.kubernetes.io/instance\"]] | join(\",\")"
}

check_apps_ha_selector_override() {
    rendered=$(rendered_or_fail 'the zone-apps HA profile renders with custom server spread selectors' \
        render_apps_ha_vouched --set-json 'server.topologySpreadConstraints=[
            {"maxSkew": 1, "topologyKey": "kubernetes.io/hostname", "whenUnsatisfiable": "DoNotSchedule", "matchLabelKeys": ["pod-template-hash"],
             "labelSelector": {"matchLabels": {"app.kubernetes.io/component": "server", "tier": "api"}}},
            {"maxSkew": 1, "topologyKey": "topology.kubernetes.io/zone", "whenUnsatisfiable": "ScheduleAnyway",
             "labelSelector": {"matchExpressions": [{"key": "tier", "operator": "In", "values": ["api"]}]}}
        ]') || return 0
    expect 'a custom spread selector keeps its labels and gains the release' \
        app.kubernetes.io/component=server,app.kubernetes.io/instance=zone-apps,tier=api "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"kubernetes.io/hostname\") | .labelSelector | $apps_labels"
    expect 'a spread selector of only expressions keeps them and gains the release' \
        'tier In api|app.kubernetes.io/instance=zone-apps' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"topology.kubernetes.io/zone\") | .labelSelector | (.matchExpressions | map(.key + \" \" + .operator + \" \" + (.values | join(\",\"))) | join(\",\")) + \"|\" + ($apps_labels)"
}

check_apps_ha_secret() {
    url='rediss://zone:p%40ss@redis.example.com:6380/0'
    rendered=$(rendered_or_fail 'secrets.create with a rediss:// URL renders' \
        render_apps_ha_created --set-string "secrets.redisUrl=$url") || return 0
    expect 'secrets.redisUrl is the secret valkey-url' "$url" "$rendered" \
        'select(.kind == "Secret") | .stringData["valkey-url"]'

    expect_renders 'a redis:// URL on a host that only starts with valkey renders' \
        render_apps_ha_created --set-string secrets.redisUrl=redis://valkey-cache.example.com:6379
    expect_renders 'a redis:// URL on an IP address renders' \
        render_apps_ha_created --set-string 'secrets.redisUrl=redis://10.0.0.5:6379'
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
        rediss://:secret@valkey.zone.svc.cluster.local.:6380; do
        expect_refused "secrets.redisUrl naming the in-cluster valkey ($url) is refused" 'in-cluster valkey service' \
            render_apps_ha_created --set-string "secrets.redisUrl=$url"
    done
    expect_refused 'the schema refuses a secrets.redisUrl scheme other than redis or rediss' "/secrets/redisUrl" \
        render_apps_ha_created --set-string secrets.redisUrl=http://redis.example.com:6379
    expect_refused 'the template refuses a secrets.redisUrl scheme other than redis or rediss' 'must start with redis:// or rediss://' \
        render_apps_ha_created --skip-schema-validation --set-string secrets.redisUrl=http://redis.example.com:6379
    expect_refused 'a ReadWriteMany claim with no storage class is refused' 'storageClass must name a ReadWriteMany storage class' \
        render_ha zone-apps --set highAvailability.externalValkey=true --set "server.trustedProxies=$trusted_proxies"
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
    rendered=$(rendered_or_fail 'HA network policy with the external Valkey CIDR and port renders' \
        render_apps_ha_vouched \
        --set networkPolicy.enabled=true \
        --set networkPolicy.externalValkeyCIDR=10.20.0.0/16 \
        --set networkPolicy.externalValkeyPort=6380) || return 0
    expect 'the server may reach the external Valkey CIDR on its port' '10.20.0.0/16 6380' "$rendered" \
        "$apps_egress | select(.to[0].ipBlock.cidr == \"10.20.0.0/16\") | .to[0].ipBlock.cidr + \" \" + (.ports[0].port | tostring)"
    expect 'the server is not sent to in-cluster valkey pods' 0 "$rendered" \
        "[$apps_egress | select(.to[0].podSelector.matchLabels[\"app.kubernetes.io/name\"] == \"valkey\")] | length"
}

check_apps_default() {
    rendered=$(rendered_or_fail 'the zone-apps defaults render' render zone-apps) || return 0
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
    expect 'the default server egress reaches the in-cluster valkey pods' 1 "$(render zone-apps --set networkPolicy.enabled=true)" \
        "[$apps_egress | select(.to[0].podSelector.matchLabels[\"app.kubernetes.io/name\"] == \"valkey\")] | length"

    expect 'the default render sets no runner name' '' "$rendered" "$apps_runner_names"

    expect_refused 'two fixed replicas sharing server.env.ZONE_RUNNER_NAME are refused' server.env.ZONE_RUNNER_NAME \
        render zone-apps --set server.autoscaling.enabled=false --set server.replicaCount=2 --set server.env.ZONE_RUNNER_NAME=zone
    expect_refused 'autoscaled replicas sharing commonEnv.ZONE_RUNNER_NAME are refused' commonEnv.ZONE_RUNNER_NAME \
        render zone-apps --set server.autoscaling.enabled=true --set commonEnv.ZONE_RUNNER_NAME=zone
    rendered=$(rendered_or_fail 'one replica with its own ZONE_RUNNER_NAME renders' \
        render zone-apps --set server.autoscaling.enabled=false --set server.replicaCount=1 --set server.env.ZONE_RUNNER_NAME=zone) || return 0
    expect 'one replica keeps its server.env.ZONE_RUNNER_NAME' zone "$rendered" "$apps_runner_names"

    rendered=$(rendered_or_fail 'a plain commonEnv.REDIS_URL renders' \
        render zone-apps --set-string commonEnv.REDIS_URL=redis://plain.example.com:6379) || return 0
    expect 'commonEnv.REDIS_URL is never a plain env var' 0 "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL" and has("value"))] | length'
}

infra_cluster='select(.kind == "Cluster" and .metadata.name == "zone-postgres")'
infra_backup='select(.kind == "ScheduledBackup" and .metadata.name == "zone-postgres")'
infra_valkey='[select(.metadata.name == "valkey" or .metadata.name == "valkey-pvc") | .kind] | join(",")'
barman_cloud_plugin=barman-cloud.cloudnative-pg.io

ai_litellm='select(.kind == "Deployment" and .metadata.name == "litellm")'
ai_ollama='select(.kind == "Deployment" and .metadata.name == "ollama")'
ai_pdb='select(.kind == "PodDisruptionBudget" and .metadata.name == "litellm")'
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
    rendered=$(rendered_or_fail 'the zone-infra defaults render' render zone-infra) || return 0
    expect 'the default cluster runs one instance' 1 "$rendered" "$infra_cluster | .spec.instances"
    expect 'the default cluster sets no postgresql block' false "$rendered" "$infra_cluster | .spec | has(\"postgresql\")"
    expect 'the default cluster leaves affinity to CNPG' false "$rendered" "$infra_cluster | .spec | has(\"affinity\")"
    expect 'the default cluster loads no plugins' false "$rendered" "$infra_cluster | .spec | has(\"plugins\")"
    expect 'the default render schedules no backup' 0 "$rendered" "[$infra_backup] | length"
    expect 'the default render deploys the in-cluster valkey' PersistentVolumeClaim,Service,Deployment "$rendered" "$infra_valkey"
}

check_infra_ha() {
    rendered=$(rendered_or_fail 'the zone-infra HA profile renders' render_ha zone-infra) || return 0
    expect 'the HA cluster runs three instances' 3 "$rendered" "$infra_cluster | .spec.instances"
    expect 'a commit waits for any one standby, and blocks without one' 'any 1 required' "$rendered" \
        "$infra_cluster | .spec.postgresql.synchronous | .method + \" \" + (.number | tostring) + \" \" + .dataDurability"
    expect 'every instance requires a node of its own' 'true required kubernetes.io/hostname' "$rendered" \
        "$infra_cluster | .spec.affinity | (.enablePodAntiAffinity | tostring) + \" \" + .podAntiAffinityType + \" \" + .topologyKey"
    expect 'the HA cluster loads no plugins without an object store' false "$rendered" "$infra_cluster | .spec | has(\"plugins\")"
    expect 'the HA render schedules no backup without an object store' 0 "$rendered" "[$infra_backup] | length"
    expect 'the HA render deploys no in-cluster valkey' '' "$rendered" "$infra_valkey"
    expect 'the HA render has no Valkey Deployment' 0 "$rendered" \
        '[select(.kind == "Deployment" and .spec.template.metadata.labels["app.kubernetes.io/name"] == "valkey")] | length'

    max_connections=$(query "$rendered" "$infra_cluster | .spec.postgresql.parameters.max_connections")
    apps=$(rendered_or_fail 'the zone-apps HA profile renders for sizing' render_apps_ha_vouched) || return 0
    ai=$(rendered_or_fail 'the zone-ai HA profile renders for sizing' render_ha zone-ai) || return 0
    server_replicas=$(query "$apps" 'select(.kind == "HorizontalPodAutoscaler" and .metadata.name == "zone-apps-server") | .spec.maxReplicas')
    litellm_replicas=$(query "$ai" "$ai_litellm | .spec.replicas")
    litellm_workers=$(query "$ai" "$ai_litellm_workers")
    case "$server_replicas$litellm_replicas$litellm_workers" in
        *[!0-9]* | '')
            fail 'the connection sizing inputs are numbers' " server $server_replicas, litellm $litellm_replicas x $litellm_workers"
            return
            ;;
    esac
    needed=$(((server_replicas + 1) * (20 + 1) + (litellm_replicas + 1) * litellm_workers * 10))
    expect_at_most 'max_connections covers every server and LiteLLM pool at the HPA maximum plus a surge pod' \
        "$max_connections" "$needed"
}

check_infra_ha_parameters() {
    rendered=$(rendered_or_fail 'the zone-infra HA profile with a numeric parameter renders' \
        render_ha zone-infra --set postgres.parameters.max_connections=500) || return 0
    expect 'a postgresql parameter set as a number renders as a string, as CNPG requires' '!!str 500' "$rendered" \
        "$infra_cluster | .spec.postgresql.parameters.max_connections | tag + \" \" + ."
}

check_infra_ha_backup() {
    rendered=$(rendered_or_fail 'the zone-infra HA profile with an object store renders' \
        render_ha zone-infra --set postgres.backup.objectStore=zone-backups) || return 0
    expect 'the cluster archives WAL through the Barman Cloud plugin' "$barman_cloud_plugin true zone-backups" "$rendered" \
        "$infra_cluster | .spec.plugins[] | .name + \" \" + (.isWALArchiver | tostring) + \" \" + .parameters.barmanObjectName"
    expect 'the scheduled backup targets the cluster through the plugin' "zone-postgres plugin $barman_cloud_plugin" "$rendered" \
        "$infra_backup | .spec | .cluster.name + \" \" + .method + \" \" + .pluginConfiguration.name"
    expect 'the scheduled backup prefers a standby' prefer-standby "$rendered" "$infra_backup | .spec.target"
    expect 'the scheduled backup has a six-field schedule' 6 "$rendered" \
        "$infra_backup | .spec.schedule | split(\" \") | length"
}

check_infra_ha_refusals() {
    expect_refused 'synchronous replication with too few standbys is refused' 'must be below postgres.instances' \
        render_ha zone-infra --set postgres.instances=1
}

check_ai_default() {
    rendered=$(rendered_or_fail 'the zone-ai defaults render' render zone-ai) || return 0
    expect 'the default LiteLLM runs one replica' 1 "$rendered" "$ai_litellm | .spec.replicas"
    expect 'the default LiteLLM sets no affinity' null "$rendered" "$ai_litellm | .spec.template.spec.affinity"
    expect 'the default render has no LiteLLM PDB' 0 "$rendered" "[$ai_pdb] | length"
}

check_ai_ha() {
    rendered=$(rendered_or_fail 'the zone-ai HA profile renders' render_ha zone-ai) || return 0
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

    litellm_workers=$(query "$rendered" "$ai_litellm_workers")
    litellm_memory=$(mebibytes "$(query "$rendered" "$ai_litellm | .spec.template.spec.containers[0].resources.limits.memory")")
    case "$litellm_workers" in
        *[!0-9]* | '') fail 'the LiteLLM worker count is a number' " got \"$litellm_workers\"" ;;
        *) expect_at_most 'the LiteLLM memory limit holds its master and every worker process' \
            "$litellm_memory" "$(((litellm_workers + 1) * litellm_process_mebibytes))" ;;
    esac

    default_ollama=$(query "$(render zone-ai)" "$ai_ollama")
    ha_ollama=$(query "$rendered" "$ai_ollama")
    if [ "$default_ollama" != "$ha_ollama" ]; then
        fail 'the HA profile leaves Ollama unchanged'
    fi
}

check_apps_default
check_apps_ha
check_apps_ha_release
check_apps_ha_selector_override
check_apps_ha_secret
check_apps_ha_trusted_proxies
check_apps_ha_runner_name
check_apps_ha_refusals
check_apps_ha_network_policy
check_infra_default
check_infra_ha
check_infra_ha_parameters
check_infra_ha_backup
check_infra_ha_refusals
check_ai_default
check_ai_ha

if [ "$failures" -gt 0 ]; then
    printf '%s Helm HA check(s) failed\n' "$failures" >&2
    exit 1
fi
printf '%s\n' 'Helm HA checks passed'

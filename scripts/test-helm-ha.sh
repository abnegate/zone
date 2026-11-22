#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
storage_class=test-rwx
failures=0

jwt_secret=0123456789abcdef0123456789abcdef
encryption_key=fedcba9876543210fedcba9876543210

render() {
    chart=$1
    shift
    helm template "$chart" "$root/helm/$chart" --namespace zone "$@"
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

render_apps_ha() {
    render_ha zone-apps --set "server.agentState.persistence.storageClass=$storage_class" "$@"
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

    expect 'the server prefers spreading across hostnames' kubernetes.io/hostname "$rendered" \
        "$apps_server | .spec.template.spec.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.topologyKey"
    expect 'the server spreads by hostname, refusing to schedule past a skew of 1' '1 DoNotSchedule Honor' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"kubernetes.io/hostname\") | (.maxSkew | tostring) + \" \" + .whenUnsatisfiable + \" \" + .nodeTaintsPolicy"
    expect 'the server spreads by zone on a best-effort basis' '1 ScheduleAnyway Honor' "$rendered" \
        "$apps_server | .spec.template.spec.topologySpreadConstraints[] | select(.topologyKey == \"topology.kubernetes.io/zone\") | (.maxSkew | tostring) + \" \" + .whenUnsatisfiable + \" \" + .nodeTaintsPolicy"
    expect 'every topology spread constraint ignores tainted and cordoned nodes' 0 "$rendered" \
        '[select(.kind == "Deployment") | .spec.template.spec.topologySpreadConstraints[] | select(.nodeTaintsPolicy != "Honor")] | length'
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
    expect 'the console prefers spreading across hostnames' kubernetes.io/hostname "$rendered" \
        "$apps_manager | .spec.template.spec.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.topologyKey"

    expect 'the ingress keeps a browser on one server replica' zone-server "$rendered" \
        'select(.kind == "Service" and .metadata.name == "zone-apps-server") | .metadata.annotations["haproxy.org/cookie-persistence"]'

    expect 'REDIS_URL is rendered once, from the secret' valkey-url "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL")] | map(.valueFrom.secretKeyRef.key // "plain") | join(",")'
    expect 'no env var named REDIS_URL carries a plain value' 0 "$rendered" \
        '[.. | select(tag == "!!map" and .name == "REDIS_URL" and has("value"))] | length'
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
        render_ha zone-apps --set highAvailability.externalValkey=true
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
    expect 'the default server keeps its own termination grace' null "$rendered" \
        "$apps_server | .spec.template.spec.terminationGracePeriodSeconds"
    expect 'the default server egress reaches the in-cluster valkey pods' 1 "$(render zone-apps --set networkPolicy.enabled=true)" \
        "[$apps_egress | select(.to[0].podSelector.matchLabels[\"app.kubernetes.io/name\"] == \"valkey\")] | length"

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
    litellm_workers=$(query "$ai" "$ai_litellm | .spec.template.spec.containers[0].args as \$arguments | \$arguments | to_entries | .[] | select(.value == \"--num_workers\") | \$arguments[.key + 1]")
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

    default_ollama=$(query "$(render zone-ai)" "$ai_ollama")
    ha_ollama=$(query "$rendered" "$ai_ollama")
    if [ "$default_ollama" != "$ha_ollama" ]; then
        fail 'the HA profile leaves Ollama unchanged'
    fi
}

check_apps_default
check_apps_ha
check_apps_ha_secret
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

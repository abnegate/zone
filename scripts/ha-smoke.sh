#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

namespace=zone
replicas=3
default_workers=3
minimum_workers=2
database_instances_ha=3
replica_budget_instances=3
cnpg_manifest=https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/4b5e244a7d031f67e025c83c1555e7726ecbbfa1/releases/cnpg-1.30.0.yaml
cache_name=zone-ha-cache
cache_image=valkey/valkey:8.1.10-alpine@sha256:b21fd94099dcd4bc6b2b9230daef69b6558b887ad4a2a1afe56ff6e745a88cdb
storage_class=zone-ha-shared
database_cluster=zone-postgres
server_deployment=zone-apps-server
litellm_deployment=litellm
server_selector=app.kubernetes.io/instance=zone-apps,app.kubernetes.io/component=server
server_port=8000
readiness_path=/health/ready
trusted_proxies='127.0.0.1/32,::1/128'
database_probe_path=/api/auth/sso/providers
failover_window=60
poll_interval=0.5
drain_timeout=300s
required_tools='docker kind kubectl helm openssl'

dry_run=false
build=false
keep_images=false
workers=$default_workers
database_instances=$database_instances_ha
pods_per_node=1
server_image=''
console_image=''
suffix=''
cluster=''
workdir=''
kubeconfig=''
cluster_created=false
drained_node=''
built_images=()
poller=''
failures=()
results=()

usage() {
    cat <<'EOF'
Usage: scripts/ha-smoke.sh [--dry-run] [--build] [--keep-images] [--workers N]
                           [--server-image IMAGE] [--console-image IMAGE]

Creates a throwaway kind cluster (zone-ha-smoke-<random>: one control plane,
three workers by default), installs zone-infra, zone-ai and zone-apps with their
HA values, and checks that the server survives a pod deletion, a rollout
restart, a CloudNativePG primary failover and a node drain while /health/ready
is polled through the Service. The cluster, the temporary directory and any
image the script built are removed on exit.

  --dry-run              Render and validate the HA charts with the kind
                         overrides, check prerequisites and print the plan.
                         Creates nothing.
  --build                Build the server and console images from this
                         checkout. The server image is the Dockerfile's
                         builder and runtime-base stages with zone-server and
                         its entrypoint, without the agent toolchains.
  --keep-images          Keep the server and console images --build made and
                         print their tags, for a later run's --server-image
                         and --console-image.
  --workers N            Run N kind workers (default 3, at least 2). Below 3,
                         Postgres runs N instances with preferred synchronous
                         durability, since each instance needs a node.
  --server-image IMAGE   Use IMAGE for zone-server instead of building it.
  --console-image IMAGE  Use IMAGE for the console instead of building it.
  -h, --help             Show this help.

Published images predate /health/ready, so a run needs --build or both images.
EOF
}

log() {
    printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"
}

die() {
    printf 'ha-smoke: %s\n' "$*" >&2
    exit 1
}

record_failure() {
    log "FAIL: $1"
    failures+=("$1")
}

parse_arguments() {
    while [ $# -gt 0 ]; do
        case $1 in
            --dry-run) dry_run=true ;;
            --build) build=true ;;
            --keep-images) keep_images=true ;;
            --workers)
                [ $# -gt 1 ] || die '--workers needs a count'
                workers=$2
                shift
                ;;
            --workers=*) workers=${1#*=} ;;
            --server-image)
                [ $# -gt 1 ] || die '--server-image needs an image'
                server_image=$2
                shift
                ;;
            --server-image=*) server_image=${1#*=} ;;
            --console-image)
                [ $# -gt 1 ] || die '--console-image needs an image'
                console_image=$2
                shift
                ;;
            --console-image=*) console_image=${1#*=} ;;
            -h | --help)
                usage
                exit 0
                ;;
            *) die "unknown argument: $1 (see --help)" ;;
        esac
        shift
    done
    set_topology
}

set_topology() {
    case $workers in
        '' | *[!0-9]*) die "--workers takes a whole number, not '$workers'" ;;
    esac
    workers=$((10#$workers))
    [ "$workers" -ge "$minimum_workers" ] || die "--workers must be at least $minimum_workers"
    if [ "$workers" -lt "$database_instances_ha" ]; then
        database_instances=$workers
    fi
    pods_per_node=$(((replicas + workers - 1) / workers))
}

check_prerequisites() {
    local tool missing=''
    for tool in $required_tools; do
        command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
    done
    [ -z "$missing" ] || die "missing prerequisites:$missing"
    command -v kubeconform >/dev/null 2>&1 || log 'kubeconform not found; rendered manifests will not be validated'
}

check_images() {
    if [ "$build" = true ]; then
        return
    fi
    if [ -z "$server_image" ] || [ -z "$console_image" ]; then
        die 'published images lack /health/ready: pass --build, or both --server-image and --console-image'
    fi
}

random_hex() {
    openssl rand -hex "$1"
}

prepare_workdir() {
    suffix=$(random_hex 4)
    cluster=zone-ha-smoke-$suffix
    local temporary=${TMPDIR:-/tmp}
    workdir=$(mktemp -d "${temporary%/}/zone-ha-smoke.XXXXXX")
    workdir=$(cd "$workdir" && pwd -P)
    kubeconfig=$workdir/kubeconfig
    export KUBECONFIG=$kubeconfig
    mkdir -p "$workdir/shared/state" "$workdir/rendered"
    chmod 0777 "$workdir/shared" "$workdir/shared/state"
    server_image=${server_image:-zone-ha-smoke/server:$suffix}
    console_image=${console_image:-zone-ha-smoke/console:$suffix}
}

image_field() {
    local image=$1 field=$2 repository tag='' digest=''
    repository=$image
    case $image in
        *@*)
            repository=${image%@*}
            digest=${image#*@}
            ;;
    esac
    case ${repository##*/} in
        *:*)
            tag=${repository##*:}
            repository=${repository%:*}
            ;;
    esac
    case $field in
        repository) printf '%s' "$repository" ;;
        tag) printf '%s' "${tag:-latest}" ;;
        digest) printf '%s' "$digest" ;;
    esac
}

image_values() {
    local image=$1 digest
    digest=$(image_field "$image" digest)
    printf '    repository: %s\n' "$(image_field "$image" repository)"
    printf '    tag: "%s"\n' "$(image_field "$image" tag)"
    if [ -n "$digest" ]; then
        printf '    digest: "%s"\n' "$digest"
    fi
    printf '    pullPolicy: IfNotPresent\n'
}

database_topology_values() {
    if [ "$database_instances" -ge "$database_instances_ha" ]; then
        return
    fi
    printf '  instances: %s\n' "$database_instances"
    printf '  synchronous:\n'
    printf '    dataDurability: preferred\n'
}

expected_budgets() {
    printf 'zone-apps-server zone-apps-manager litellm'
    if [ "$database_instances" -ge "$replica_budget_instances" ]; then
        printf ' zone-postgres'
    fi
    printf ' zone-postgres-primary'
}

write_overrides() {
    local database_password superuser_password litellm_key
    database_password=$(random_hex 16)
    superuser_password=$(random_hex 16)
    litellm_key=sk-$(random_hex 16)

    cat >"$workdir/cluster.yaml" <<EOF
apiVersion: v1
kind: Namespace
metadata:
  name: $namespace
---
apiVersion: v1
kind: Secret
metadata:
  name: zone-secrets
  namespace: $namespace
type: Opaque
stringData:
  username: zone
  password: "$database_password"
  litellm-master-key: "$litellm_key"
  litellm-salt-key: "$(random_hex 16)"
  litellm-database-url: "postgres://zone:$database_password@$database_cluster-rw:5432/litellm"
---
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: $storage_class
provisioner: kubernetes.io/no-provisioner
reclaimPolicy: Retain
volumeBindingMode: Immediate
---
apiVersion: v1
kind: PersistentVolume
metadata:
  name: $storage_class
spec:
  capacity:
    storage: 1Gi
  accessModes:
    - ReadWriteMany
  storageClassName: $storage_class
  persistentVolumeReclaimPolicy: Retain
  hostPath:
    path: /shared/state
    type: Directory
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: $cache_name
  namespace: $namespace
spec:
  replicas: 1
  selector:
    matchLabels:
      app.kubernetes.io/name: $cache_name
  template:
    metadata:
      labels:
        app.kubernetes.io/name: $cache_name
    spec:
      containers:
        - name: valkey
          image: $cache_image
          args: ["--save", "", "--appendonly", "no"]
          ports:
            - name: redis
              containerPort: 6379
          readinessProbe:
            tcpSocket:
              port: redis
            periodSeconds: 2
---
apiVersion: v1
kind: Service
metadata:
  name: $cache_name
  namespace: $namespace
spec:
  selector:
    app.kubernetes.io/name: $cache_name
  ports:
    - name: redis
      port: 6379
      targetPort: redis
EOF

    cat >"$workdir/zone-infra.yaml" <<EOF
postgres:
  superuser:
    create: true
    password: "$superuser_password"
  storage:
    size: 1Gi
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: "1"
      memory: 1Gi
$(database_topology_values)
EOF

    cat >"$workdir/zone-ai.yaml" <<'EOF'
ollama:
  init:
    enabled: false
  persistence:
    size: 1Gi
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: "1"
      memory: 2Gi
litellm:
  workers: 1
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: "1"
      memory: 3Gi
EOF

    cat >"$workdir/zone-apps.yaml" <<EOF
secrets:
  create: true
  dbPassword: "$database_password"
  jwtSecret: "$(random_hex 32)"
  encryptionKey: "$(random_hex 32)"
  litellmMasterKey: "$litellm_key"
  redisUrl: redis://$cache_name:6379
server:
  trustedProxies: "$trusted_proxies"
  image:
$(image_values "$server_image")
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: "1"
      memory: 1Gi
  agentState:
    persistence:
      storageClass: $storage_class
      size: 1Gi
manager:
  image:
$(image_values "$console_image")
  resources:
    requests:
      cpu: 50m
      memory: 64Mi
    limits:
      cpu: 500m
      memory: 512Mi
EOF
    chmod 0600 "$workdir"/*.yaml
}

render_charts() {
    local chart
    for chart in zone-infra zone-ai zone-apps; do
        helm template "$chart" "$root/helm/$chart" --namespace "$namespace" \
            -f "$root/helm/$chart/values-ha.yaml" -f "$workdir/$chart.yaml" \
            >"$workdir/rendered/$chart.yaml" || die "helm refused to render $chart with the HA values"
    done
    cp "$workdir/cluster.yaml" "$workdir/rendered/cluster.yaml"
    log "rendered zone-infra, zone-ai and zone-apps with values-ha.yaml and the kind overrides"
}

validate_rendered() {
    if ! command -v kubeconform >/dev/null 2>&1; then
        return
    fi
    kubeconform -strict -summary -ignore-missing-schemas "$workdir"/rendered/*.yaml ||
        die 'kubeconform rejected the rendered manifests'
}

image_source() {
    case $1 in
        zone-ha-smoke/*:"$suffix")
            if [ "$build" = true ]; then
                printf 'built by --build'
            else
                printf 'a run needs --build or an explicit image'
            fi
            ;;
        *) printf 'given' ;;
    esac
}

database_durability() {
    if [ "$database_instances" -ge "$database_instances_ha" ]; then
        printf 'required'
    else
        printf 'preferred'
    fi
}

kept_images_plan() {
    if [ "$keep_images" = true ]; then
        printf 'the server and console images --build makes'
    else
        printf 'none'
    fi
}

print_plan() {
    cat <<EOF
Plan:
  cluster     $cluster (1 control plane, $workers workers, no host ports; kubeconfig kept in a temporary directory)
  shared      a temporary host directory at /shared on every node, behind the hostPath ReadWriteMany class $storage_class
  operator    CloudNativePG from $cnpg_manifest
  database    $database_instances Postgres instances ($(database_durability) synchronous durability)
  cache       $cache_name ($cache_image) standing in for a managed Valkey
  images      server $server_image ($(image_source "$server_image"))
              console $console_image ($(image_source "$console_image"))
  charts      zone-infra, zone-ai, zone-apps with values-ha.yaml and the kind overrides
              (server.trustedProxies $trusted_proxies: the smoke has no ingress controller)
  wait        $replicas Ready server pods, at most $pods_per_node per node, zone_cluster_bus_connected 1 on each
  disruptions with $readiness_path polled through the Service every ${poll_interval}s:
              delete a server pod; kubectl rollout restart (at most $pods_per_node server pods per node after it);
              fail over the Postgres primary (server pods reach the new primary within ${failover_window}s);
              drain a worker running a server pod but not the primary (--timeout=$drain_timeout), then uncordon it
  budgets     $(expected_budgets)
  images kept $(kept_images_plan)
EOF
}

build_server_image() {
    local builder=zone-ha-smoke/server-builder:$suffix base=zone-ha-smoke/server-base:$suffix
    built_images+=("$builder" "$base" "$server_image")
    log "building $server_image"
    docker build -f "$root/manager/Dockerfile" --target builder -t "$builder" "$root"
    docker build -f "$root/manager/Dockerfile" --target runtime-base -t "$base" "$root"
    cat >"$workdir/server.Dockerfile" <<'EOF'
ARG BUILDER
ARG BASE
FROM ${BUILDER} AS builder

FROM ${BASE}
RUN groupadd --system --gid 999 zone \
    && useradd --system --uid 999 --gid zone --create-home --home-dir /home/zone --shell /bin/bash zone
WORKDIR /app
COPY --from=builder /build/bin/zone-server /app/zone-server
COPY --from=builder /build/runner/zone_server/migrations /app/migrations
COPY manager/docker-entrypoint.sh /usr/local/bin/manager-entrypoint.sh
RUN mkdir -p /app/workspace /app/agent-state /app/artifacts /app/embed-cache /tmp/repos \
    && chown -R zone:zone /app/workspace /app/agent-state /app/artifacts /app/embed-cache /tmp/repos \
    && chmod 0700 /app/agent-state /tmp/repos \
    && chmod 0755 /usr/local/bin/manager-entrypoint.sh
EXPOSE 8000
ENTRYPOINT ["/usr/local/bin/manager-entrypoint.sh"]
CMD ["/app/zone-server"]
EOF
    docker build -f "$workdir/server.Dockerfile" --build-arg "BUILDER=$builder" --build-arg "BASE=$base" \
        -t "$server_image" "$root"
}

build_console_image() {
    built_images+=("$console_image")
    log "building $console_image"
    docker build -f "$root/manager/frontend/Dockerfile" -t "$console_image" "$root"
}

build_images() {
    if [ "$build" != true ]; then
        return
    fi
    case $server_image in
        zone-ha-smoke/server:"$suffix") build_server_image ;;
    esac
    case $console_image in
        zone-ha-smoke/console:"$suffix") build_console_image ;;
    esac
}

kind_config() {
    printf '%s/kind-config.yaml' "$workdir"
}

with_workers() {
    awk -v count="$1" '
        function emit(   copy) {
            for (copy = 0; copy < count; copy++) {
                printf "%s", block
            }
            emitted = 1
        }
        /^  - role: / || /^[^ ]/ || /^$/ {
            if (inside && !emitted) {
                emit()
            }
            inside = 0
        }
        /^  - role: worker$/ {
            inside = 1
            capturing = block == ""
        }
        inside {
            if (capturing) {
                block = block $0 "\n"
            }
            next
        }
        { print }
        END {
            if (inside && !emitted) {
                emit()
            }
        }
    '
}

write_kind_config() {
    local config count
    config=$(kind_config)
    sed "s|\${ZONE_HA_SHARED_DIR}|$workdir/shared|g" "$root/k8s/kind-ha-config.yaml" | with_workers "$workers" >"$config"
    count=$(grep -c '^  - role: worker$' "$config" || true)
    [ "$count" -eq "$workers" ] || die "the generated kind config has $count workers, not $workers"
}

create_cluster() {
    if kind get clusters 2>/dev/null | grep -qx "$cluster"; then
        die "kind cluster $cluster already exists"
    fi
    cluster_created=true
    log "creating kind cluster $cluster"
    kind create cluster --name "$cluster" --config "$(kind_config)" --kubeconfig "$kubeconfig" --wait 180s
}

worker_nodes() {
    kind get nodes --name "$cluster" | grep -v -- '-control-plane$' | paste -sd, -
}

load_images() {
    local image workers
    workers=$(worker_nodes)
    for image in "$server_image" "$console_image"; do
        if docker image inspect "$image" >/dev/null 2>&1; then
            log "loading $image into $workers"
            kind load docker-image --name "$cluster" --nodes "$workers" "$image"
        else
            log "$image is not local; the nodes will pull it"
        fi
    done
}

install_operator() {
    log 'installing CloudNativePG'
    kubectl apply --server-side -f "$cnpg_manifest" >/dev/null
    kubectl rollout status deployment/cnpg-controller-manager -n cnpg-system --timeout=300s
}

install_charts() {
    kubectl apply -f "$workdir/cluster.yaml" >/dev/null
    kubectl rollout status "deployment/$cache_name" -n "$namespace" --timeout=300s

    wait_until 120 install_chart zone-infra || die 'zone-infra never installed'
    log 'waiting for the Postgres cluster'
    kubectl wait "clusters.postgresql.cnpg.io/$database_cluster" -n "$namespace" \
        --for=condition=Ready --timeout=900s
    check_database_instances 'after install'
    kubectl wait databases.postgresql.cnpg.io/zone-manager -n "$namespace" \
        --for=jsonpath='{.status.applied}'=true --timeout=300s

    install_chart zone-ai
    install_chart zone-apps
    log "waiting for $replicas Ready server pods, at most $pods_per_node per node"
    wait_until 900 servers_settled ||
        die "the server never settled on $replicas Ready pods with at most $pods_per_node per node"
    kubectl rollout status "deployment/$litellm_deployment" -n "$namespace" --timeout=600s ||
        die 'LiteLLM never became available'
}

install_chart() {
    log "installing $1"
    helm upgrade --install "$1" "$root/helm/$1" --namespace "$namespace" \
        -f "$root/helm/$1/values-ha.yaml" -f "$workdir/$1.yaml" >/dev/null
}

wait_until() {
    local timeout=$1 deadline
    shift
    deadline=$(($(date +%s) + timeout))
    until "$@"; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
            return 1
        fi
        sleep 2
    done
}

server_pods() {
    kubectl get pods -n "$namespace" -l "$server_selector" -o go-template='{{range .items}}{{.metadata.name}} {{or .spec.nodeName "-"}} {{if .metadata.deletionTimestamp}}Terminating{{else}}{{range .status.conditions}}{{if eq .type "Ready"}}{{.status}}{{end}}{{end}}{{end}}{{"\n"}}{{end}}'
}

ready_server_pods() {
    server_pods | awk '$3 == "True" { print $1 }'
}

servers_ready() {
    local pods
    pods=$(server_pods) || return 1
    awk -v wanted="$replicas" '
        { total++ }
        $3 == "True" { ready++ }
        END { exit !(total == wanted && ready == wanted) }
    ' <<<"$pods"
}

servers_spread() {
    ready_server_pods_by_node | awk -v limit="$pods_per_node" '
        { count[$2]++ }
        END {
            for (node in count) {
                if (count[node] > limit) {
                    exit 1
                }
            }
        }
    '
}

ready_server_pods_by_node() {
    server_pods | awk '$3 == "True" { print $1, $2 }'
}

servers_settled() {
    servers_ready && servers_spread
}

check_spread() {
    local placement
    placement=$(ready_server_pods_by_node | tr '\n' ';')
    if servers_spread; then
        log "no node holds more than $pods_per_node of the $replicas server pods $1: $placement"
    else
        record_failure "a node holds more than $pods_per_node of the $replicas server pods $1: $placement"
    fi
}

server_restarts() {
    kubectl get pods -n "$namespace" -l "$server_selector" \
        -o go-template='{{range .items}}{{.metadata.name}}{{range .status.containerStatuses}} {{.restartCount}}{{end}}{{"\n"}}{{end}}' |
        sort
}

pod_proxy() {
    printf '/api/v1/namespaces/%s/pods/%s:%s/proxy' "$namespace" "$1" "$server_port"
}

service_proxy() {
    printf '/api/v1/namespaces/%s/services/http:%s:http/proxy' "$namespace" "$server_deployment"
}

bus_connected() {
    local metrics
    metrics=$(kubectl get --raw "$(pod_proxy "$1")/metrics" 2>/dev/null) || return 1
    grep -qx 'zone_cluster_bus_connected 1' <<<"$metrics"
}

check_bus() {
    local pod
    for pod in $(ready_server_pods); do
        if wait_until 90 bus_connected "$pod"; then
            log "$pod reports zone_cluster_bus_connected 1"
        else
            record_failure "$pod never reported zone_cluster_bus_connected 1 $1"
        fi
    done
}

reaches_database() {
    kubectl get --raw "$(pod_proxy "$1")$database_probe_path" >/dev/null 2>&1
}

servers_reach_database() {
    local pod
    for pod in $(ready_server_pods); do
        reaches_database "$pod" || return 1
    done
}

check_budgets() {
    local budget allowed
    for budget in $(expected_budgets); do
        if kubectl get pdb "$budget" -n "$namespace" >/dev/null 2>&1; then
            log "PodDisruptionBudget $budget exists"
        else
            record_failure "PodDisruptionBudget $budget is missing"
        fi
    done
    allowed=$(kubectl get pdb "$server_deployment" -n "$namespace" -o jsonpath='{.status.disruptionsAllowed}' 2>/dev/null || true)
    if [ "$allowed" = 1 ]; then
        log "PodDisruptionBudget $server_deployment allows 1 disruption"
    else
        record_failure "PodDisruptionBudget $server_deployment allows ${allowed:-no} disruptions, not 1"
    fi
}

poll_readiness() {
    local output=$1 stop=$2 path answer
    path="$(service_proxy)$readiness_path"
    while [ ! -e "$stop" ]; do
        if answer=$(kubectl get --raw "$path" --request-timeout=3s 2>&1); then
            printf 'ok\n' >>"$output"
        else
            poll_failure "$answer" >>"$output"
        fi
        sleep "$poll_interval"
    done
}

poll_failure() {
    local message
    message=$(printf '%s' "$1" | tr '\n' ' ' | cut -c1-160)
    case $message in
        'Unable to connect to the server'*) printf 'unmeasured %s\n' "$message" ;;
        *) printf 'fail %s\n' "$message" ;;
    esac
}

tally() {
    awk '
        { total++ }
        /^fail/ { failed++ }
        /^fail.*draining/ { draining++ }
        /^unmeasured/ { unmeasured++ }
        END { printf "%d %d %d %d", total, failed, draining, unmeasured }
    ' "$1"
}

samples() {
    if [ -f "$1" ]; then
        wc -l <"$1" | tr -d ' '
    else
        printf '0'
    fi
}

has_samples() {
    [ "$(samples "$1")" -ge "$2" ]
}

stop_poller() {
    if [ -n "$poller" ]; then
        kill "$poller" 2>/dev/null || true
        wait "$poller" 2>/dev/null || true
        poller=''
    fi
}

measure() {
    local name=$1 strict=$2 action=$3 output stop summary total failed draining unmeasured before
    output=$workdir/$name.poll
    stop=$workdir/$name.stop
    poll_readiness "$output" "$stop" &
    poller=$!
    wait_until 30 has_samples "$output" 3 || die "the readiness poller never answered before $name"

    log "disruption: $name"
    "$action"

    before=$(samples "$output")
    wait_until 30 has_samples "$output" $((before + 3)) || true
    touch "$stop"
    wait "$poller" || true
    poller=''

    read -r total failed draining unmeasured <<<"$(tally "$output")"
    summary="$name: $total polls, $failed failed ($draining answered draining), $unmeasured never reached the API server"
    results+=("$summary")
    log "$summary"
    if [ "$((failed + unmeasured))" -gt 0 ]; then
        grep -v '^ok' "$output" | sort | uniq -c | sort -rn | awk 'NR <= 5'
    fi
    if [ "$strict" = strict ] && [ "$failed" -gt 0 ]; then
        record_failure "$name: $failed readiness polls failed"
    elif [ "$((failed - draining))" -gt 0 ]; then
        record_failure "$name: $((failed - draining)) readiness polls found no Ready server"
    fi
}

pod_gone() {
    [ -z "$(kubectl get pod "$1" -n "$namespace" --ignore-not-found -o name 2>/dev/null)" ]
}

delete_server_pod() {
    local pod
    pod=$(ready_server_pods)
    pod=${pod%%$'\n'*}
    log "deleting $pod"
    kubectl delete pod "$pod" -n "$namespace" --wait=false >/dev/null
    wait_until 180 pod_gone "$pod" || die "$pod never went away"
    wait_until 300 servers_ready || die "the server never returned to $replicas Ready pods after $pod was deleted"
    check_spread "after $pod was deleted"
}

restart_servers() {
    kubectl rollout restart "deployment/$server_deployment" -n "$namespace" >/dev/null
    if ! kubectl rollout status "deployment/$server_deployment" -n "$namespace" --timeout=600s; then
        die 'kubectl rollout restart never completed'
    fi
    wait_until 300 servers_ready || die "the server never returned to $replicas Ready pods after the rollout restart"
    check_spread 'after the rollout restart'
}

current_primary() {
    kubectl get "clusters.postgresql.cnpg.io/$database_cluster" -n "$namespace" -o jsonpath='{.status.currentPrimary}'
}

primary_moved() {
    local primary
    primary=$(current_primary 2>/dev/null) || return 1
    [ -n "$primary" ] && [ "$primary" != "$1" ]
}

database_ready() {
    kubectl wait "clusters.postgresql.cnpg.io/$database_cluster" -n "$namespace" \
        --for=condition=Ready --timeout=5s >/dev/null 2>&1
}

fail_over_database() {
    local primary standby restarts disrupted started elapsed
    primary=$(current_primary)
    restarts=$(server_restarts)
    disrupted=$(date +%s)
    if kubectl cnpg version >/dev/null 2>&1; then
        standby=$(kubectl get pods -n "$namespace" -l "cnpg.io/cluster=$database_cluster,cnpg.io/instanceRole=replica" \
            -o jsonpath='{.items[0].metadata.name}')
        log "promoting $standby over $primary"
        kubectl cnpg promote "$database_cluster" "$standby" -n "$namespace"
    else
        log "deleting the Postgres primary $primary"
        kubectl delete pod "$primary" -n "$namespace" --wait=false >/dev/null
    fi
    wait_until 600 primary_moved "$primary" || die "Postgres never elected a primary other than $primary"
    started=$(date +%s)
    log "$(current_primary) is the new Postgres primary after $((started - disrupted))s"

    if wait_until "$failover_window" servers_reach_database; then
        elapsed=$(($(date +%s) - started))
        log "every server pod reached the new primary within ${elapsed}s"
    else
        record_failure "a server pod could not reach the new primary within ${failover_window}s"
    fi
    wait_until 600 database_ready || die 'the Postgres cluster never recovered from the failover'
    servers_ready || record_failure "the server lost a Ready pod during the database failover"
    if [ "$(server_restarts)" != "$restarts" ]; then
        record_failure "server pods restarted during the database failover"
    fi
}

ready_database_instances() {
    kubectl get "clusters.postgresql.cnpg.io/$database_cluster" -n "$namespace" -o jsonpath='{.status.readyInstances}'
}

database_instances_ready() {
    local ready
    ready=$(ready_database_instances 2>/dev/null) || return 1
    [ "$ready" = "$database_instances" ]
}

check_database_instances() {
    if wait_until 600 database_instances_ready; then
        log "the Postgres cluster has $database_instances Ready instances $1"
    else
        record_failure "the Postgres cluster has $(ready_database_instances 2>/dev/null || true) Ready instances $1, not $database_instances"
    fi
}

pod_node() {
    kubectl get pod "$1" -n "$namespace" -o jsonpath='{.spec.nodeName}'
}

drain_candidate() {
    local primary_node
    primary_node=$(pod_node "$(current_primary)") || return 1
    [ -n "$primary_node" ] || return 1
    ready_server_pods_by_node | awk -v primary="$primary_node" '$2 != primary { print $2; exit }'
}

servers_left_node() {
    server_pods | awk -v node="$1" '$2 == node { found = 1 } END { exit found }'
}

uncordon_node() {
    if [ -z "$drained_node" ]; then
        return
    fi
    if kubectl uncordon "$drained_node" >/dev/null; then
        log "uncordoned $drained_node"
        drained_node=''
    else
        log "could not uncordon $drained_node"
    fi
}

drain_node() {
    local node
    node=$(drain_candidate) || die 'could not find the node of the Postgres primary'
    [ -n "$node" ] || die 'no worker runs a Ready server pod without the Postgres primary'
    drained_node=$node
    log "draining $node"
    if kubectl drain "$node" --ignore-daemonsets --delete-emptydir-data --timeout="$drain_timeout"; then
        log "drained $node"
    else
        record_failure "kubectl drain $node did not finish within $drain_timeout"
    fi
    wait_until 300 servers_ready || record_failure "the server never returned to $replicas Ready pods while $node was drained"
    servers_left_node "$node" || record_failure "a server pod is still on $node after the drain"
    uncordon_node
    check_database_instances "after $node was uncordoned"
}

show_diagnostics() {
    log 'diagnostics'
    kubectl get pods -A -o wide
    kubectl get events -n "$namespace" --sort-by=.lastTimestamp | tail -40
    kubectl logs -n "$namespace" -l "$server_selector" --prefix --tail=40
    kubectl get pods -n "$namespace" -o jsonpath='{range .items[*]}{.metadata.name} last exited {.status.containerStatuses[*].lastState.terminated.exitCode} {.status.containerStatuses[*].lastState.terminated.reason}{"\n"}{end}'
    kubectl logs -n "$namespace" -l app.kubernetes.io/name=litellm --prefix --tail=30 --previous
}

delete_cluster() {
    case $cluster in
        zone-ha-smoke-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]) ;;
        *)
            log "refusing to delete a cluster named $cluster"
            return
            ;;
    esac
    log "deleting kind cluster $cluster"
    docker exec "$cluster-control-plane" sh -c 'rm -rf /shared/state' >/dev/null 2>&1
    kind delete cluster --name "$cluster" --kubeconfig "$kubeconfig"
}

kept() {
    [ "$keep_images" = true ] && { [ "$1" = "$server_image" ] || [ "$1" = "$console_image" ]; }
}

remove_built_images() {
    local image
    for image in ${built_images[@]+"${built_images[@]}"}; do
        if kept "$image"; then
            log "kept image $image"
        else
            docker image rm "$image" >/dev/null 2>&1 && log "removed image $image"
        fi
    done
}

print_kept_images() {
    local image
    for image in ${built_images[@]+"${built_images[@]}"}; do
        if kept "$image"; then
            printf 'Rerun with the kept images: scripts/ha-smoke.sh --server-image %s --console-image %s\n' \
                "$server_image" "$console_image"
            return
        fi
    done
}

cleanup() {
    local status=$?
    set +e
    stop_poller
    if [ "$cluster_created" = true ]; then
        if [ "$status" -ne 0 ]; then
            show_diagnostics
        fi
        uncordon_node
        delete_cluster
    fi
    remove_built_images
    print_kept_images
    if [ -n "$workdir" ]; then
        rm -rf "$workdir" || log "could not remove $workdir"
    fi
    exit "$status"
}

report() {
    local line
    printf '\nResults:\n'
    for line in ${results[@]+"${results[@]}"}; do
        printf '  %s\n' "$line"
    done
    if [ ${#failures[@]} -gt 0 ]; then
        printf 'Failures:\n'
        for line in "${failures[@]}"; do
            printf '  %s\n' "$line"
        done
        return 1
    fi
    printf 'HA smoke passed.\n'
}

run() {
    docker info >/dev/null 2>&1 || die 'docker is not reachable'
    build_images
    create_cluster
    load_images
    install_operator
    install_charts
    check_bus 'after install'
    wait_until 60 servers_reach_database || die 'the server pods never reached Postgres'
    check_budgets
    measure 'pod deletion' tolerant delete_server_pod
    measure 'rollout restart' tolerant restart_servers
    measure 'database failover' strict fail_over_database
    measure 'node drain' strict drain_node
    check_bus 'after the disruptions'
    report
}

main() {
    parse_arguments "$@"
    check_prerequisites
    if [ "$dry_run" = false ]; then
        check_images
    fi
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    prepare_workdir
    write_kind_config
    write_overrides
    render_charts
    validate_rendered
    print_plan
    if [ "$dry_run" = true ]; then
        log 'dry run: nothing was created'
        return
    fi
    run
}

main "$@"

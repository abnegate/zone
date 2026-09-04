#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

port_in_use() {
    lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1
}

pick_port() {
    local port
    for port in "$@"; do
        if ! port_in_use "$port"; then
            echo "$port"
            return 0
        fi
    done
    echo "$1"
}

KIND_HTTP_PORT="${KIND_HTTP_PORT:-$(pick_port 80 8080 18080)}"
KIND_HTTPS_PORT="${KIND_HTTPS_PORT:-$(pick_port 443 8443 18443)}"
KIND_STATS_PORT="${KIND_STATS_PORT:-$(pick_port 31024 31025)}"
export KIND_HTTP_PORT KIND_HTTPS_PORT KIND_STATS_PORT

KIND_CONFIG="$(mktemp)"
trap 'rm -f "$KIND_CONFIG"' EXIT
# envsubst is not always present; fall back to sed.
if command -v envsubst >/dev/null 2>&1; then
    envsubst '${KIND_HTTP_PORT} ${KIND_HTTPS_PORT} ${KIND_STATS_PORT}' \
      < "$PROJECT_ROOT/k8s/kind-config.yaml" > "$KIND_CONFIG"
else
    sed -e "s/\${KIND_HTTP_PORT}/${KIND_HTTP_PORT}/g" \
        -e "s/\${KIND_HTTPS_PORT}/${KIND_HTTPS_PORT}/g" \
        -e "s/\${KIND_STATS_PORT}/${KIND_STATS_PORT}/g" \
        "$PROJECT_ROOT/k8s/kind-config.yaml" > "$KIND_CONFIG"
fi

echo -e "${BLUE}Creating Kind cluster for Zone...${NC}"
echo -e "${BLUE}Host ports:${NC} HTTP=${KIND_HTTP_PORT} HTTPS=${KIND_HTTPS_PORT} stats=${KIND_STATS_PORT}"

if kind get clusters 2>/dev/null | grep -q "^zone-dev$"; then
    echo -e "${YELLOW}Kind cluster 'zone-dev' already exists. Delete it first with 'make kind-delete' or continue with existing cluster.${NC}"
    if [ -t 0 ]; then
        read -p "Continue with existing cluster? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    else
        echo "Running in non-interactive mode, continuing with existing cluster..."
    fi
else
    echo -e "${GREEN}Creating Kind cluster...${NC}"
    kind create cluster --config "$KIND_CONFIG"
fi

echo -e "${GREEN}Waiting for cluster to be ready...${NC}"
kubectl wait --for=condition=Ready nodes --all --timeout=120s

# Kind ships a default local-path StorageClass named "standard". Only install
# rancher/local-path-provisioner when the cluster has no default class.
if kubectl get storageclass -o jsonpath='{range .items[?(@.metadata.annotations.storageclass\.kubernetes\.io/is-default-class=="true")]}{.metadata.name}{"\n"}{end}' | grep -q .; then
    echo -e "${GREEN}Default StorageClass already present; skipping local-path-provisioner install.${NC}"
else
    echo -e "${GREEN}Installing local-path-provisioner...${NC}"
    kubectl apply -f https://raw.githubusercontent.com/rancher/local-path-provisioner/49b2be8e26d6d34c9afaa21fa33108d2e82f8955/deploy/local-path-storage.yaml
    kubectl patch storageclass local-path -p '{"metadata": {"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'
fi

echo -e "${GREEN}Installing CloudNativePG operator...${NC}"
kubectl apply --server-side -f \
  https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/4b5e244a7d031f67e025c83c1555e7726ecbbfa1/releases/cnpg-1.30.0.yaml

echo -e "${GREEN}Waiting for CNPG operator...${NC}"
kubectl wait --namespace cnpg-system \
  --for=condition=ready pod \
  --selector=app.kubernetes.io/name=cloudnative-pg \
  --timeout=180s

echo -e "${GREEN}Installing HAProxy Ingress Controller...${NC}"
helm repo add haproxytech https://haproxytech.github.io/helm-charts 2>/dev/null || true
helm repo update haproxytech

helm upgrade --install haproxy-ingress haproxytech/kubernetes-ingress \
  --version 1.54.0 \
  --namespace haproxy-controller --create-namespace \
  --set controller.kind=DaemonSet \
  --set controller.daemonset.useHostPort=true \
  --set controller.daemonset.hostPorts.http=80 \
  --set controller.daemonset.hostPorts.https=443 \
  --set controller.daemonset.hostPorts.stat=31024 \
  --set controller.service.enabled=false \
  --set controller.ingressClass=haproxy \
  --set controller.ingressClassResource.default=true

echo -e "${GREEN}Waiting for HAProxy Ingress Controller...${NC}"
kubectl wait --namespace haproxy-controller \
  --for=condition=ready pod \
  --selector=app.kubernetes.io/name=kubernetes-ingress \
  --timeout=180s

echo -e "${GREEN}Creating zone namespace...${NC}"
kubectl create namespace zone --dry-run=client -o yaml | kubectl apply -f -

echo ""
echo -e "${GREEN}Kind cluster 'zone-dev' is ready!${NC}"
echo -e "${BLUE}Installed:${NC}"
echo "  - Kind built-in local-path storage (or rancher local-path fallback)"
echo "  - CloudNativePG operator"
echo "  - HAProxy Ingress Controller"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo "  Run 'make tilt-up' to start development"
if [[ "$KIND_HTTP_PORT" != "80" ]]; then
    echo -e "${YELLOW}Host port 80 is in use; ingress is on http://127.0.0.1:${KIND_HTTP_PORT}${NC}"
fi

#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}Creating Kind cluster for Zone...${NC}"

# Create local storage directory
mkdir -p ~/.zone/data

# Check if cluster already exists
if kind get clusters 2>/dev/null | grep -q "zone-dev"; then
    echo -e "${YELLOW}Kind cluster 'zone-dev' already exists. Delete it first with 'make kind-delete' or continue with existing cluster.${NC}"
    # Check if running in non-interactive mode
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
    # Create cluster
    echo -e "${GREEN}Creating Kind cluster...${NC}"
    kind create cluster --config "$PROJECT_ROOT/k8s/kind-config.yaml"
fi

# Wait for cluster to be ready
echo -e "${GREEN}Waiting for cluster to be ready...${NC}"
kubectl wait --for=condition=Ready nodes --all --timeout=120s

# Install local-path-provisioner for persistent storage
echo -e "${GREEN}Installing local-path-provisioner...${NC}"
kubectl apply -f https://raw.githubusercontent.com/rancher/local-path-provisioner/v0.0.26/deploy/local-path-storage.yaml

# Set default storage class
kubectl patch storageclass local-path -p '{"metadata": {"annotations":{"storageclass.kubernetes.io/is-default-class":"true"}}}'

# Install CNPG operator
echo -e "${GREEN}Installing CloudNativePG operator...${NC}"
kubectl apply --server-side -f \
  https://raw.githubusercontent.com/cloudnative-pg/cloudnative-pg/release-1.25/releases/cnpg-1.25.0.yaml

# Wait for CNPG operator to be ready
echo -e "${GREEN}Waiting for CNPG operator...${NC}"
kubectl wait --namespace cnpg-system \
  --for=condition=ready pod \
  --selector=app.kubernetes.io/name=cloudnative-pg \
  --timeout=120s

# Install HAProxy Ingress Controller
echo -e "${GREEN}Installing HAProxy Ingress Controller...${NC}"
helm repo add haproxytech https://haproxytech.github.io/helm-charts 2>/dev/null || true
helm repo update haproxytech

helm upgrade --install haproxy-ingress haproxytech/kubernetes-ingress \
  --version 1.47.1 \
  --namespace haproxy-controller --create-namespace \
  --set controller.kind=DaemonSet \
  --set controller.daemonset.useHostPort=true \
  --set controller.service.type=NodePort \
  --set controller.service.nodePorts.stat=31024 \
  --set controller.ingressClass=haproxy \
  --set controller.ingressClassResource.default=true

# Wait for HAProxy ingress controller
echo -e "${GREEN}Waiting for HAProxy Ingress Controller...${NC}"
kubectl wait --namespace haproxy-controller \
  --for=condition=ready pod \
  --selector=app.kubernetes.io/name=kubernetes-ingress \
  --timeout=120s

# Create zone namespace
echo -e "${GREEN}Creating zone namespace...${NC}"
kubectl create namespace zone --dry-run=client -o yaml | kubectl apply -f -

echo ""
echo -e "${GREEN}Kind cluster 'zone-dev' is ready!${NC}"
echo -e "${BLUE}Installed:${NC}"
echo "  - local-path-provisioner (default storage class)"
echo "  - CloudNativePG operator"
echo "  - HAProxy Ingress Controller"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo "  Run 'make tilt-up' to start development"

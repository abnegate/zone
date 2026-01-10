#!/bin/bash
set -e

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${YELLOW}Deleting Kind cluster 'zone-dev'...${NC}"

if kind get clusters 2>/dev/null | grep -q "zone-dev"; then
    kind delete cluster --name zone-dev
    echo -e "${GREEN}Kind cluster deleted.${NC}"
else
    echo -e "${YELLOW}Kind cluster 'zone-dev' does not exist.${NC}"
fi

# Optionally clean up local data
read -p "Also delete local data in ~/.zone/data? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    rm -rf ~/.zone/data
    echo -e "${GREEN}Local data deleted.${NC}"
fi

#!/bin/bash
set -euo pipefail

BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}Deleting Kind cluster 'zone-dev'...${NC}"

if kind get clusters 2>/dev/null | grep -q "^zone-dev$"; then
    kind delete cluster --name zone-dev
    echo -e "${GREEN}Kind cluster deleted.${NC}"
else
    echo -e "${YELLOW}Kind cluster 'zone-dev' does not exist.${NC}"
fi

delete_data() {
    rm -rf "${HOME}/.zone/data"
    echo -e "${GREEN}Local data deleted.${NC}"
}

if [[ "${KIND_DELETE_DATA:-}" =~ ^[Yy1]$ ]]; then
    delete_data
elif [ -t 0 ]; then
    read -p "Also delete local data in ~/.zone/data? [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        delete_data
    fi
else
    echo -e "${YELLOW}Skipping local data deletion (non-interactive). Set KIND_DELETE_DATA=1 to remove ~/.zone/data.${NC}"
fi

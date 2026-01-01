#!/bin/bash
# Run Gleam tests with code coverage analysis
#
# Usage:
#   ./scripts/test-coverage.sh           # Run with coverage
#   ./scripts/test-coverage.sh -v        # Run with verbose output
#
# Output:
#   - coverage/index.html       Summary report
#   - coverage/*.html           Per-module detail reports
#   - coverage/coverage.coverdata  Raw coverage data

set -e

cd "$(dirname "$0")/.."

# Set test database environment variables (defaults for local dev)
export TEST_POSTGRES_HOST="${TEST_POSTGRES_HOST:-localhost}"
export TEST_POSTGRES_PORT="${TEST_POSTGRES_PORT:-5433}"
export TEST_POSTGRES_DB="${TEST_POSTGRES_DB:-zone}"
export TEST_POSTGRES_USER="${TEST_POSTGRES_USER:-zone}"
export TEST_POSTGRES_PASSWORD="${TEST_POSTGRES_PASSWORD:-zone}"

# Ensure the project is built
echo "Building project..."
gleam build

# Run the coverage script
escript scripts/coverage.escript "$@"

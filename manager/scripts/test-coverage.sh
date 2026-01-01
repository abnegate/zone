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

# Ensure the project is built
echo "Building project..."
gleam build

# Run the coverage script
escript scripts/coverage.escript "$@"

#!/bin/bash
# Install git hooks for the voiz repository

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Installing git hooks..."

# Configure git to use our hooks directory
git config core.hooksPath .githooks

echo "Git hooks installed successfully!"
echo "Pre-commit hook will now run automatically before each commit."
echo ""
echo "To uninstall, run: git config --unset core.hooksPath"

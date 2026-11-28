#!/usr/bin/env bash
# Print the highest published vX.Y.Z release tag of a repository, leaving out
# the tag given as the second argument when there is one.
# Usage: highest-release.sh <owner/repo> [excluded-tag]
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: highest-release.sh <owner/repo> [excluded-tag]" >&2
  exit 2
fi

repository=$1
excluded=${2:-}

gh api "repos/${repository}/releases" --paginate \
  --jq '.[] | select((.draft or .prerelease) | not) | .tag_name' |
  grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' |
  grep -vxF -- "${excluded:-^}" |
  sort -V |
  tail -n 1 || true

#!/usr/bin/env bash
# Commit the baked runner/Cargo.toml / Cargo.lock bump to the default branch
# when RELEASE_SHA is still that branch's tip.
# Usage: commit-release-version.sh
# Required env: VERSION, DEFAULT_BRANCH, RELEASE_SHA
set -euo pipefail

: "${VERSION:?set VERSION}"
: "${DEFAULT_BRANCH:?set DEFAULT_BRANCH}"
: "${RELEASE_SHA:?set RELEASE_SHA}"

files=(
  runner/Cargo.toml
  runner/Cargo.lock
  runner/zone_desktop/tauri.conf.json
)
if git diff --quiet -- "${files[@]}"; then
  echo "workspace.package.version already ${VERSION}"
  exit 0
fi

git fetch origin "${DEFAULT_BRANCH}"
tip="$(git rev-parse "origin/${DEFAULT_BRANCH}")"
if [ "${RELEASE_SHA}" != "$tip" ]; then
  echo "release commit is not ${DEFAULT_BRANCH} tip; skipping version commit"
  exit 0
fi

git config user.name "github-actions[bot]"
git config user.email "github-actions[bot]@users.noreply.github.com"
git switch -C "${DEFAULT_BRANCH}"
git add "${files[@]}"
git commit -m "(chore): set package.version to ${VERSION}"
git push origin "HEAD:${DEFAULT_BRANCH}"

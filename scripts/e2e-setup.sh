#!/bin/sh
# Install what the Playwright suite needs: the workspace's packages and the
# browser builds Playwright drives. Pass browser names to narrow it, e.g.
# `scripts/e2e-setup.sh chromium`.
#
# Separate from `make test-e2e` because this is the only part that reaches the
# network. The suite itself mocks every API call and serves the console from
# vite, so once this has run the tests need nothing else — and it needs running
# again only when the pinned Playwright version changes.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

if [ "$#" -eq 0 ]; then
    set -- chromium firefox webkit
fi

if ! command -v bun >/dev/null 2>&1; then
    echo "bun is not on PATH. Install it from https://bun.sh, then run this again." >&2
    exit 1
fi

# Large tarballs — playwright-core among them — fail to download at bun's
# default concurrency on some networks, which leaves the workspace without the
# very package this script is for. The failure reads as an integrity error
# rather than a timeout, so it looks like a corrupt package instead of a
# crowded pipe. Each attempt asks for fewer parallel downloads.
installed=
for concurrency in default 8 2 1; do
    if [ "$concurrency" = default ]; then
        echo "==> bun install"
        if bun install; then
            installed=yes
            break
        fi
    else
        echo "==> bun install --network-concurrency $concurrency"
        if bun install --network-concurrency "$concurrency"; then
            installed=yes
            break
        fi
    fi
    echo "    Some packages did not download. Retrying with fewer parallel downloads."
done

if [ -z "$installed" ]; then
    echo "bun install could not download every package. The errors above say which." >&2
    exit 1
fi

# The workspace's own binary, so the browser builds match the pinned version.
# `bunx playwright` resolves the latest release instead, which downloads one
# build and then fails to launch it looking for another.
playwright=manager/frontend/node_modules/.bin/playwright
if [ ! -x "$playwright" ]; then
    echo "$playwright is missing even though bun install reported success." >&2
    exit 1
fi

echo "==> $("$playwright" --version), installing: $*"
"$playwright" install "$@"

cat <<'READY'

Ready. The browsers live in ~/.cache/ms-playwright and outlive this checkout.

  make test-e2e                               every spec, every browser
  cd manager/frontend && bun run test:e2e     the same thing
  cd manager/frontend && ./node_modules/.bin/playwright test \
      e2e/chats.e2e.ts --project=chromium -g "Create Chat"

manager/frontend/e2e/CONTEXT.md says what the suite does and does not prove.
READY

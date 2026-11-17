# Console end-to-end tests

Playwright drives the real console in a real browser. Every API call is mocked
(`routeApi` in `fixtures.ts`) and the suite signs itself in with a forged token,
so **it needs no server, no database and no model** — only vite and a browser.

## Setup

```sh
make test-e2e-setup             # chromium, firefox and webkit
make test-e2e-setup ARGS=chromium
```

That is the only step that reaches the network. It installs the workspace's
packages and the browser builds for this platform into `~/.cache/ms-playwright`,
which outlives the checkout, so a container that has done it once does not
repeat it until the pinned Playwright version changes.

## Running

```sh
make test-e2e                                    # every spec, every browser
cd manager/frontend && bun run test:e2e          # the same thing

# One file, one engine, one describe — what you want while fixing a spec.
cd manager/frontend && ./node_modules/.bin/playwright test \
    e2e/chats.e2e.ts --project=chromium -g "Create Chat"
```

`playwright.config.ts` starts the dev server itself (`bun start`, port 3001
locally and 4174 in CI) and reuses one already listening there outside CI.
CI shards each engine four ways, so a job named `Tests (chromium 1/4)` is one
quarter of the specs rather than a browser-specific failure.

Call the workspace's own binary rather than `bunx playwright`: `bunx` resolves
the latest release, which downloads one browser build and then fails to launch
it looking for a different one.

The run needs no `bun run build:ui`. `vite.config.ts` aliases `@zone/ui` to its
source in both serve and build mode, so the `dist` that `pretest` makes is for
the unit tests only.

## What a pass does and does not prove

It proves the console renders, routes and reacts the way a person would see —
against answers the suite wrote itself. It says nothing about the server. A
suite that mocks every call can pass while nothing works, which is what
`manager/frontend/live` and `make live-verify` are for; CONTRIBUTING.md covers
them. A feature is not verified until it has been seen against a real server.

## Verified platforms

Playwright ships builds per distribution. All three engines download and launch
on Debian 13 arm64 (`firefox-debian-13-arm64`, `webkit-debian-13-arm64`) with
no root and no `playwright install-deps`, which is what Zone's own runtime is.
Confirmed 2026-11-17 against the pinned 1.62.1: chromium 151.0.7922.34,
firefox 153.0, webkit 26.5.

## If `bun install` reports failed packages

Large tarballs fail to download at bun's default concurrency on some networks,
and report an integrity error rather than a timeout — so it reads as a corrupt
package when it is a crowded pipe. `scripts/e2e-setup.sh` retries with fewer
parallel downloads, which clears it; the same flag is worth reaching for on any
`bun install` here:

```sh
bun install --network-concurrency 2
```

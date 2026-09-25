# Port puller into zone as "Pull Requests" and "Releases" nav items

## Context

Puller (`~/Local/puller`) is a single-user local dashboard: a Node server driving the `gh` CLI plus a React + Tailwind/shadcn front end. It shows the user's open GitHub pull requests bucketed into Ready / In progress / Not ready (review threads resolved, bot review current for the head commit, CI green), with diff, commits, failed-check logs and a guarded admin merge; and recent releases with their release-workflow pipeline state plus a create-release flow (next patch tag, generated-notes preview, tag + release with read-back). It also launches local coding-agent CLIs (Fix / Auto / conflict repair / New Task / Verify) in worktrees under `~/Local`.

Zone is a multi-tenant self-hosted platform (Rust axum + Postgres, React console with its own `@zone/ui` kit and `--ui-*` token CSS). It already has typed GitHub clients (`zone_vcs::PrService`), the readiness and release-pipeline domain logic (`agent/readiness.rs`, `agent/releases.rs`), a private GitHub client with rate limiting inside `agent/integrations.rs` (the `assess_pull_requests` / `assess_release_pipelines` LLM tools), per-workspace GitHub sources holding encrypted tokens, and a task runner that can act on PRs. What it lacks is any console page or REST route for PRs or releases.

Goal: two first-class sidebar pages, "Pull Requests" and "Releases", served by zone's Rust backend over zone's stored source credentials, rebuilt in zone's design system, gated by zone permissions, tested the way zone tests things. Puller's local-CLI agent features are mapped onto zone's task machinery as a follow-up slice or dropped.

Puller is read-only reference; its uncommitted WIP (Chat/Discord/Tauri, a compact Fix button) does not change readiness, merge or release semantics.

## Decisions

| Question | Decision | Why |
|---|---|---|
| What the PR page lists | Every open PR across the workspace's active `github` sources, sorted `updated_at` desc, with client-side Repository and Author filters | Zone has no GitHub identity per console user (`users` has no login column); source tokens belong to the workspace, not the viewer. "Mine" becomes a default filter once a profile field exists. |
| Readiness rule | Reuse `agent/readiness.rs::assess` unchanged, with each source's `review_signals` (default: every recognised bot, Greptile + CodeRabbit) | It already generalises puller's Greptile-only rule. Zone is stricter in two places (drafts never ready, no checks ⇒ `checks_absent`); keep zone's rule since the auto-project pipeline shares it. |
| Buckets | `ready` if `assessment.ready`; else `in_progress` if `checks.state == pending`; else `not_ready` | Port of puller `groupPulls` minus the agent-run terms (slice 2 adds "linked task has an active run"). |
| Releases listed | Every non-draft release published in the last N days (default 7, allowed 1..90) per active source; PR links parsed from the notes; no "contains my PRs" constraint | Puller's catalogue was seeded from the viewer's PRs; in zone the catalogue is the sources. Drafts must be filtered before `release_identity` (null `published_at` errors today). |
| Caching | Per-source in-memory snapshots on `AppState` (TTL, single-flight, stale-on-error, rate-limit backoff, incremental re-assessment); no housekeeping sweep; no Postgres table | One assessment is 1 GraphQL + 3 paged REST calls per PR; puller's 10 s cadence would burn a PAT's 5k/h. Sweeps can't be poked and run for nobody. The cache type is small enough to swap for a table later. |
| Permissions | New `pull_requests:read`, `pull_requests:update` (merge), `releases:read`, `releases:create`, seeded by migration 049 like `022`; admin and user roles get all four, viewer the two reads | Matches zone's `resource:action` convention; viewers can look without merging. Per-request tenancy stays with `workspace_members` (`WorkspaceMember` / `WorkspaceWriter`). |
| Layout | Pull Requests = Projects master-detail (320 px `.card--list` pane + `<aside>` detail); Releases = Tasks single scroll body with day groups | Detail pane gives diff/commits/logs one stable scroller without puller's motion/row-continuity machinery; releases cards are self-contained. |
| Nav position | After Tasks: Chats, Projects, Tasks, **Pull Requests**, **Releases**, Sources, Search, Models, Wiki, Organization, Workspace | Plan → work → review/merge → ship reads as the delivery lifecycle; settings stay last. |
| Merge | Server re-assesses the single PR, refuses unless ready and the expected head matches, then `PrService::merge` (REST, GraphQL admin fallback on 405 when `ZONE_GITHUB_ADMIN_MERGE`); method from the request (`merge` default, `squash`, `rebase`) | Puller's `readinessComplete` + `--match-head-commit` guard. |
| Create release | preview (generate-notes) → digest; create re-previews, requires digest match, then tag object → ref → draft release → publish → read-back; never deletes; 409 `release_manual_reconciliation_required` on partial failure; one create per source at a time | Puller's transaction, with tagger `Zone <zone@users.noreply.github.com>` and marker `<!-- zone-release:<uuid> -->`. |
| Dropped | Local CLI agents, Auto dispatcher (Web Locks), New Task worktrees, release Verify, Chat/Discord overlays, Tauri, keyboard-shortcut system, motion/row continuity, Shiki worker, action-token/Origin scheme, IndexedDB transcripts, favourites/hidden prefs, SectionPager | Zone has its own runner, auto projects, task wizard, desktop client, JWT+CORS, task logs. Favourites/hidden and syntax highlighting are cheap follow-ups. |

Name collisions to avoid: `crate::pull` / `PullRegistry` / `/ws/pull` / frontend `PullProvider` are Ollama model pulls; `workers::pr` publishes PRs from runs; `agent::releases` is pipeline logic. Use `services::github`, `routes::pull_requests`, `routes::releases`, frontend `features/pull-requests`, `features/releases`, hooks `usePullRequests*`.

## API contract (settled; the frontend `schemas.ts` files are the single adapt point)

All under `require_auth`; JSON; errors `{ success: false, error }` (409 refusals add `code`). Reads use `WorkspaceMember`, writes `WorkspaceWriter`. Only UUIDs, numbers and SHAs appear in paths (metrics label folding); tags travel in bodies/queries.

| Method | Path | Response |
|---|---|---|
| GET | `/api/workspaces/{workspace_id}/pull-requests?refresh=1` | `{ generated_at, stale, partial, ttl_secs, refresh_floor_secs, counts: {ready, in_progress, not_ready, total}, sources: [SourceStatus], pull_requests: [PullRequest], warnings: [] }` |
| GET | `.../pull-requests/{source_id}/{number}` | `{ pull_request: PullRequest, threads: [Thread], comments: [BotComment] }` (unresolved threads with comments; latest recognised-bot summaries) |
| GET | `.../pull-requests/{source_id}/{number}/diff?head=<sha40>` | `{ diff: Diff }`; 409 `head_moved` |
| GET | `.../pull-requests/{source_id}/{number}/commits?head=` | `{ commits: { number, head, base, complete, commits: [Commit] } }` |
| GET | `.../pull-requests/{source_id}/{number}/commits/{sha}?head=` | `{ diff: Diff }` |
| GET | `.../pull-requests/{source_id}/{number}/jobs/{job_id}/log?head=` | `{ log: { job_id, run_id, head, text, truncated, fetched_at } }` |
| POST | `.../pull-requests/{source_id}/{number}/merge` `{ expected_head, method? }` | `{ merged: { number, sha, admin, url } }`; 409 `code: head_moved \| not_ready \| conflict \| protected` (+ `current_head` / `blockers`) |
| GET | `/api/workspaces/{workspace_id}/releases?days=7&refresh=1` | `{ generated_at, stale, partial, window_days, ttl_secs, sources: [SourceStatus], releases: [Release], warnings }` |
| GET | `.../releases/options?refresh=1` | `{ generated_at, sources: [SourceOptions], warnings }` |
| GET | `.../releases/{source_id}/{release_id}/pipeline?refresh=1` | `{ pipeline: ReleasePipeline, state, succeeded, complete, blockers }` |
| POST | `.../releases/preview` `{ source_id, tag, expected_latest_tag }` | `{ preview: Preview }`; 400 invalid tag; 409 `latest_tag_moved` / `tag_exists` |
| POST | `.../releases` `{ source_id, tag, expected_latest_tag, prerelease, digest }` | 201 `{ release: { source_id, id, tag, name, url, published_at } }`; 409 `preview_stale` / `release_in_progress` / `release_manual_reconciliation_required { tag, tag_object, reference_created, release_id }` |

Row shapes (snake_case; existing Rust `Serialize` output is reused verbatim, so `PipelineState` stays kebab-case: `timed-out`, `action-required`):

- `PullRequest`: `source_id, repository ("owner/name"), repository_url, number, title, url, author, updated_at, draft, head, head_ref, base_ref, readiness ('ready'|'in_progress'|'not_ready'), blockers: [{code, detail}] (BlockerReport), checks: {state, complete, counts: {total, passed, failed, running, queued, in_progress, unknown}} (CheckSummary), check_details: [{kind: 'workflow'|'check_run'|'status', id, name, state, url, updated_at}], reviews: [ReviewSummary {reviewer, comment_id, url, confidence: {score, scale}|null, reviewed_commit, created_at}], review_current, unresolved_threads, threads_complete, comments_complete, assessed_at, rank`. `check_details[].id` for `kind == 'check_run'` is the Actions job id the log route takes.
- `SourceStatus`: `source_id, name, repository, repository_url, freshness ('cached'|'refreshed'|'floored'|'stale'), generated_at, warning`.
- `Thread`: `id, path, line, outdated, url, comments: [{id, author, body, url, created_at}]`. `BotComment`: `reviewer, body, url, created_at`.
- `Diff`: `number, head, base, complete, warning, files: [{path, previous_path, status, additions, deletions, changes, binary, truncated, blob_url, hunks: [{header, old_start, old_lines, new_start, new_lines, lines: [{kind: 'addition'|'deletion'|'context'|'meta', content, old_line, new_line}]}]}]` (puller's `PullDiff`, parsed server-side from GitHub's per-file `patch`).
- `Commit`: `sha, message, author_name, author_login, authored_at, url`.
- `Release`: `source_id, repository, repository_url, id, tag, name, url, published_at, prerelease, pulls: [{number, url, title}], pipeline: ReleasePipeline {release, lookup, runs: [PipelineRun], checked_at}, state, succeeded, complete, blockers`.
- `SourceOptions`: `source_id, repository, repository_url, default_branch, latest_tag, next_tag, previous_tags`. `Preview`: `source_id, repository, tag, name, body, base_tag, target, pulls, digest`.

Polling contract: PR list every 30 s while visible plus on focus (`refetchIntervalInBackground: false`); the Refresh button sends `refresh=1` and is disabled inside `refresh_floor_secs`; back off 10/20/40/80/120 s on 502. Releases list every 120 s; per-release `pipeline` every 15 s only for releases younger than 30 min whose lookup is pending or a run is queued/running, patched into the list cache. After a merge: invalidate `['pull-requests', ws]` and `['releases', ws]`; after a create: `['releases', ws]`, `['releases', ws, 'options']`, `['pull-requests', ws]`. The server invalidates the source's snapshots on both writes.

## Backend (Rust, `runner/zone_server`)

### Module map

```
src/services/github/
  mod.rs            GithubServices (AppState field): pulls / releases / options / pipelines Snapshots, releasing guard, publisher: PrService
  origin.rs         api_origin(&str) -> Result<Url, OriginError>            CodeQL request-url barrier; validated once, returned as a value
  credentials.rs    connected(state, ws, source_id) / connected_all(state, ws) -> ConnectedSource { id, name, owner, repository, configuration }
                    the ONE source-credential decrypt (replaces copies in integrations.rs::run, wait.rs::github, planner.rs::source_credential)
  client.rs         Github + Configuration + rate limiting MOVED from agent/integrations.rs, plus typed methods (below)
  snapshot.rs       Snapshots<K, T>: per-key TTL, refresh floor, single-flight, stale-on-error, retry_after on RATE_LIMIT_EXHAUSTED
  diff.rs           parse_patch(&str) -> Vec<Hunk>; budget(files, limit) -> (Vec<DiffFile>, complete)   (port of puller diff.mjs)
  pull_requests.rs  PullRequestService: list / detail / diff / commits / commit_diff / job_log / merge; bucket(); refresh_plan()
  releases.rs       ReleaseService: recent / pipeline / options / preview / create; Tag; next_patch_tag(); previous_tags(); pulls_in_notes(); digest()
src/routes/pull_requests.rs, src/routes/releases.rs      handlers in the routes/sync.rs style, registered in routes/mod.rs protected_routes
migrations/049_pull_request_and_release_permissions.sql
tests/pull_requests_tests.rs, tests/releases_tests.rs     integration against an axum fake GitHub on 127.0.0.1:0 (pattern: tests/pr_reception_status_tests.rs)
```

`agent/integrations.rs` keeps `Operation`, `Integration`, `Arguments`, the `Tool` impl, `run`/`observe`, JSON shaping (`assessment_record`, `pipeline_record`, `bound_*`, citations, log excerpting) and re-exports `Configuration`, `Github`, `SETTLED_ASSESSMENTS` from `services::github::client` so `agent/wait.rs` compiles unchanged. Its `readiness` / `release_pipelines` / `check_logs` become thin adapters over the typed methods with identical JSON output, so the existing wiremock tests keep passing (they move with the client; build with `Github::new(configuration, Url::parse(&format!("{}/", server.uri())))`). `agent/readiness.rs` and `agent/releases.rs` stay put; `services::github` imports items from them (never the module `agent::releases` as an alias).

### Typed client additions (`client.rs`)

`Github::new(configuration, origin: Url)` (replaces the `ORIGIN` constant; `url()` and `graphql()` already join correctly for `https://api.github.com` and `https://ghe/api/v3`). New thin wrappers over the existing `get`/`post`/`pages`, plus `patch` and `get_optional` (404 → `None`):

- PRs: `open_pulls(limit) -> (Vec<PullRecord>, complete)`, `pull(number)`, `assess(&PullRecord, Depth::{Listing, Full}) -> Assessed { assessment, risk, checks: Vec<CheckRecord> }` (Listing skips the 30-page changed-files walk; Full keeps it for the LLM tool), `changed_files(number, max_pages) -> (Vec<FileRecord>, complete)`, `commits(number)` (cap 250), `commit_files(&CommitSha)`, `job_log(job_id, expected_head, limit_bytes) -> JobLogText` (existing verify_job/log_response/collect_log, tail kept).
- Releases: `releases_since(since, limit)` (drafts dropped, stops past the cutoff), `release(id)`, `release_by_tag(tag)`, `pipelines(&[ReleaseIdentity]) -> Vec<ReleasePipeline>` (today's sweep + exact fallback), `pipeline(&ReleaseIdentity)`, `default_branch()`, `commit_sha(reference)`, `tags(max_pages)`, `generate_notes(tag, target, previous_tag)`, `tag_exists(tag)`, `create_tag_object(tag, message, object, &Tagger)`, `create_reference(reference, sha)`, `create_draft_release(ReleaseDraft)`, `publish_release(id)`.
- `RATE_LIMIT_EXHAUSTED` becomes `pub const` so `Snapshots` can recognise it.

### Services

- `Snapshots::read(key, refresh, now, fetch: FnOnce(Option<Arc<T>>) -> Future<Result<T, String>>) -> Result<(Snapshot<T>, Freshness), String>`: fresh and not refresh → `Cached`; refresh inside the floor → `Floored`; `retry_after` in the future → `Stale` without fetching; else take the per-entry async mutex, re-check, fetch (previous value passed in for incremental work), replace on Ok; Err with a previous value → `Stale` + warning (+ `retry_after = now + 5 min` on rate-limit exhaustion); Err with nothing → Err. `peek`, `invalidate`.
- `PullRequestService::list`: `connected_all` → per-source `snapshots.read` in parallel → `refresh_plan(previous, listed, now, force)` re-assesses only PRs whose `updated_at`/head changed, whose checks are pending/unknown/incomplete, or whose checks failed and were assessed > 120 s ago (puller's `FAILURE_REFRESH_INTERVAL`); bucket, sort (bucket, `updated_at` desc), rank. `merge`: `pull` → head guard → `assess(Listing)` → refuse `NotReady { blockers }` unless ready → `publisher.merge(&PullRequestReference { owner, repository, number }, token, Some(&node_id), head, title, message, method, config.github.admin_merge)` → map `PrError::{HeadMoved, NotMergeable, Protected, AuthFailed}` → invalidate the source's `pulls` + `releases` snapshots. `diff`/`commits`/`job_log` all take `expected_head` and answer 409 `head_moved` when the PR moved.
- `ReleaseService::recent`: per source `releases_since(now - days)` → `pulls_in_notes` (regex `https://github\.com/{owner}/{repo}/pull/(\d+)`, titles from the generated-notes line, dedup) → `pipelines` merged into the previous snapshot with `agent::releases::merge`. `options`: `tags(3 pages)` + `default_branch` → `next_patch_tag` (highest stable `v?X.Y.Z`, bump patch, keep prefix, default `v0.1.0`) + `previous_tags` (top 10). `preview`: latest-tag check → `tag_exists` → `default_branch` → `commit_sha` → `generate_notes` → `pulls_in_notes` → `digest` (sha256 over canonical `{repository.lowercase, tag, name, body, base_tag, target.lowercase, pulls[{number,title,url}]}`). `create`: `releasing` guard → re-preview and compare digests → tag object → ref → draft → publish → read-back (`draft == false`, `tag_name == tag`) → invalidate `releases`, `options`, `pulls`; any failure after the ref exists → `ReconciliationRequired` naming exactly what was written.

### Config, state, errors, permissions

- `config.rs`: `GithubConfig { pull_request_ttl_secs (45, 10..3600), release_ttl_secs (120, 10..3600), release_window_days (7, 1..90), open_pull_request_limit (50, 1..200), admin_merge (true) }` with `Default` + `from_env()` (`ZONE_GITHUB_*` via `env_u64` / `env_truthy`); `Config.github` set in `from_env`, listed in the hand-written `Debug`, and `github: Default::default()` at the 7 literal `Config` sites (`config.rs` tests, `state.rs` ×2, `tests/common/mod.rs` ×2, `tests/sync_tests.rs`, `tests/message_embedding_tests.rs`). Pipeline TTL (15 s), options TTL (300 s), refresh floor (10 s), diff budget (16 MiB) and log budget (existing `MAX_LOG_BYTES`) are named constants in the services. Touchpoints: `.env.example` (`ZONE_GITHUB_*` block after `ZONE_AUTO_*`), `docker-compose.yml` + `docker-compose.dev.yml` manager `environment:` lists, `docs/CONFIGURATION.md` (new "GitHub pages" section; also document `GITHUB_API_URL`, currently missing).
- `state.rs`: `github: GithubServices` built by `GithubServices::from_config(&config)` in all three constructors; accessor `github()`.
- `error.rs`: add `ServerError::Upstream(String)` → 502 (the `into_response` match is exhaustive). Domain refusals return `Ok((StatusCode::CONFLICT, Json(..)).into_response())` with a `code`, like `routes/tasks.rs::create_run`.
- `migrations/049_pull_request_and_release_permissions.sql`: `SET LOCAL lock_timeout = '5s'`; `INSERT INTO permissions … ON CONFLICT (name) DO NOTHING` for the four names; `role_permissions` for admin (`…0001`) and user (`…0002`) `WHERE resource IN ('pull_requests','releases')`, viewer (`…0003`) `AND action = 'read'`. No `BEGIN;`/`COMMIT;`. Also append the four names to the hard-coded admin list in `db/users.rs::get_user_with_permissions` (admins bypass `role_permissions`; without this they are locked out).
- `db/audit.rs`: `actions::PULL_REQUEST_MERGED`, `actions::RELEASE_CREATED`, `resources::PULL_REQUEST`, `resources::RELEASE`; handlers audit via `routes::common::audit` after success.
- `.github/codeql/extensions/zone-models/models/barriers.yml`: `["zone_server::services::github::origin::api_origin", "ReturnValue", "request-url", "manual"]` (and `client::Github::log_redirect` if CodeQL flags the log redirect).
- `zone_vcs::PrService::configured` builds `Client::new()` with no timeout; add a 30 s timeout there so a stalled GitHub cannot hang the merge handler (dev hot reload does not watch `zone_vcs`, restart the container).
- No new crates (`sha2`, `hex`, `regex`, `dashmap`, `wiremock` exist), so `cargo deny` is unaffected. `.sqlx` offline cache untouched (runtime `query_as` only).

### Ordered steps

1. Move the client to `services/github/client.rs` with `pub use` shims; full suite green with no behaviour change (own commit).
2. `origin.rs` + `Github::new(configuration, origin)`; update callers and moved tests; barriers row.
3. `credentials.rs`; rewrite the three decrypt copies on it.
4. `GithubConfig` + 7 sites + `.env.example` / compose / `CONFIGURATION.md`.
5. `ServerError::Upstream`; `PrService` timeout.
6. `snapshot.rs`, `GithubServices`, `AppState.github`.
7. Typed client methods with wiremock tests; re-express the tool adapters (same JSON).
8. `diff.rs` + tests.
9. `pull_requests.rs` + tests. 10. `releases.rs` + tests.
11. Route modules, registration, audit constants, route unit tests.
12. Migration 049 + `db/users.rs` + permission tests (`tests/tenant_admin_permissions_tests.rs`).
13. Integration tests against the fake GitHub.
14. `cargo fmt`, clippy `-D warnings`, nextest, doc tests.

## Frontend (`manager/frontend`, React 19 + react-query + zod + Biome; `@zone/ui` kit; plain CSS with `--ui-*` tokens, no Tailwind utilities in app TSX)

### Wiring

- `src/shared/types/permissions.ts`: `PULL_REQUESTS: { READ: 'pull_requests:read', UPDATE: 'pull_requests:update' }`, `RELEASES: { READ: 'releases:read', CREATE: 'releases:create' }`.
- `src/shared/components/Sidebar/Sidebar.tsx`: two `navItems` between Tasks and Sources, single-`d` 24×24 stroke-2 icons: `{ path: '/pull-requests', label: 'Pull Requests', icon: 'M9 6a3 3 0 11-6 0 3 3 0 016 0zM6 9v12M13 6h3a2 2 0 012 2v7M21 18a3 3 0 11-6 0 3 3 0 016 0z' }` and `{ path: '/releases', label: 'Releases', icon: 'M7 7h.01M7 3h5c.512 0 1.024.195 1.414.586l7 7a2 2 0 010 2.828l-7 7a2 2 0 01-2.828 0l-7-7A1.994 1.994 0 013 12V7a4 4 0 014-4z' }` (Heroicons v1 outline `tag`).
- `src/App.tsx`: routes `pull-requests` and `releases` after `tasks`, wrapped in `ProtectedRoute requiredPermission={PERMISSIONS.PULL_REQUESTS.READ}` / `RELEASES.READ`; write actions gated in-page with `PermissionGate`.
- `src/api/pullRequests.ts`, `src/api/releases.ts` in the `api/tasks.ts` shape (`setGetAccessToken`, `getHeaders`, `parseErrorResponse`, `parse(Schema, json)`; every segment `encodeURIComponent`); registered in `Client.setAccessToken` (`src/api/client.ts`); check `client.auth.test.ts` / `client.test.ts` for sub-API assertions.
- `src/shared/utils/time.ts`: `relativeTime(value)` via date-fns `formatDistanceToNow` + `absoluteTime(value)` for `title` tooltips (first file in `shared/utils`).

### `features/pull-requests/`

```
schemas.ts (+test) · types.ts · index.ts
utils/identity.ts (pullRequestKey / parsePullRequestKey "sourceId:number"), grouping.ts (READINESS_ORDER/LABELS/TINTS, groupPullRequests), status.ts (CI_TINTS, CHECK_TINTS, summarizeChecks, cardBadge), checks.ts (failedChecks with job ids)
hooks/usePullRequests.ts (list; refetchInterval 30 s, refetchIntervalInBackground false; refresh() calls the API with refresh=1 and setQueryData; exposes stale/warnings/floor)
hooks/usePullRequestDetail.ts · usePullRequestDiff.ts · usePullRequestCommits.ts (+ useCommitDiff) · useCheckLog.ts · useMergePullRequest.ts
      keys ['pull-requests', ws, 'detail'|'diff'|'commits'|'commit'|'log', source_id, number, head, …]; artifact queries keyed on head so a force-push invalidates; enabled only when mounted/asked
components/PullRequestList.tsx (sticky group headers with count Badge + cards) · PullRequestCard.tsx · PullRequestDetail.tsx · PullRequestBlockers.tsx (failed checks + "Show logs", threads, bot review) · PullRequestDiff.tsx (collapsible files, plain mono unified hunks, batches of 20) · PullRequestCommits.tsx (40 px rows; selecting loads its diff) · MergeDialog.tsx (kit Modal; method select; window.confirm throws in tests) · PullRequestsSkeleton.tsx · icons.tsx
pages/PullRequestsPage.tsx (+test) · pages/PullRequestsPage.css · pages/index.ts
```

Page: `div.page.page--workspace.pull-requests-page` → `PageBar title="Pull Requests" subtitle="Open pull requests and their readiness"` with `Tabs` (All / Ready / In progress / Not ready), Repository and Author `<select>`s, Refresh icon button → `div.pull-requests-workspace` with the 320 px list pane (stale/warning notice strip, group headers, 72 px cards: repo + `#number`, readiness Badge, title, checks summary with status dot, relative time) and the detail `<aside>` (`.details-header` 48 px, `.details-content` with `dl.detail-facts` + blockers list + `Tabs` Blockers / Diff / Commits, `.details-actions` 48 px: "Open on GitHub", Merge gated by `PULL_REQUESTS.UPDATE`, disabled unless ready). Deep link `?id=<source_id>:<number>` honoured once, cleared on close; selection cleared if the PR disappears. Empty/filtered/error states via `EmptyState` and `.error-banner`.

### `features/releases/`

```
schemas.ts (+test) · types.ts · index.ts
utils/grouping.ts (dayLabel Today/Yesterday/weekday, groupReleasesByDay), pipeline.ts (PIPELINE_TINTS/LABELS, pipelineActive), tags.ts (tagProblem)
hooks/useReleases.ts (list every 120 s while visible) · useReleasePipeline.ts (15 s for active releases < 30 min, setQueryData into the list) · useReleaseOptions.ts (enabled only while the wizard is open) · useCreateRelease.ts (preview + create mutations, invalidations)
components/ReleaseDayGroup.tsx · ReleaseCard.tsx (name link, tag Badge mono, repository, relative time, pipeline chips, "N PRs" + expand toggle → 40 px included-PR rows) · ReleasePipelineChips.tsx · CreateReleaseWizard.tsx (kit Wizard, 2 steps: Target = source select + tag input prefilled with next_tag + previous tags line + Pre-release Checkbox; Review = preview facts, included PRs, notes via MessageContent links="all" or <pre>; Publish → create with digest; 409 → "Reload options") · ReleasesSkeleton.tsx · icons.tsx
pages/ReleasesPage.tsx (+test) · pages/ReleasesPage.css · pages/index.ts
```

Page: `PageBar title="Releases" subtitle="Recent releases and their pipelines"` with Repository `<select>`, Refresh, `New release` (gated by `RELEASES.CREATE`) → `div.page-body.releases-body` with partial-history notice, sticky 28 px day headers, 72 px release card headers.

### CSS rules every new stylesheet must satisfy (pinned by `src/styles/design.test.ts` and `pages.design.test.ts`)

Only `--ui-*` tokens (no hex, no `rgba(`, no legacy `--gray-*` scales); tints via `color-mix(in srgb, var(--ui-x) N%, transparent)`; dark overrides under `:root[data-theme='dark']`; never restyle `.page-bar` (48 px); controls 32 / 28 px; badges 20 px (`--ui-badge-height`); rows 40 / 56 px (`--ui-list-row`, `--ui-list-row-2`); list cards `height: 4.5rem; box-sizing: border-box; padding: var(--ui-space-2) var(--ui-space-3)` with a 20 + 18 + 16 line stack; reuse `.details-header` / `.details-content` / `.detail-facts` / `.details-actions` from ProjectsPage.css rather than redefining them; no gradients, `translateY` lifts or decorative `::before` on cards (skeleton shimmer excepted); mono via `--ui-font-mono` at `--ui-text-xs` with 20 px lines; sticky headers stick inside the pane/body scroller only; `@media (max-width: 768px)` hides the list pane while a detail is open. Add `describe('pull requests page layout')` and `describe('releases page layout')` blocks to `pages.design.test.ts` pinning the 320 px pane, 72 px cards, 28 px sticky group/day headers, 20 px chip row, 40 px rows, diff line grid, and palette-only colours.

### Tests

- Unit (bun, `bun run test`): api modules (URLs, bearer header, encoded segments, bodies, error envelope); schemas (fixtures, enum fallbacks, defaults); utils (grouping, status, checks, day labels, pipeline tints, tag problems); hooks (polling flags, refresh path, enabled gating, invalidations); components (group counts, card badges, detail lazy fetching, blockers/logs, diff markers and batching, commits, merge dialog, release card chips/expand, wizard flow); pages (heading, skeleton, empty/filtered, tabs and selects, deep link, stale notice, refresh); `Sidebar.test.tsx` (new items, links, 11-item order); `pages.design.test.ts` blocks.
- Playwright (mocked): `e2e/navigation.e2e.ts` (`toHaveCount(11)`, re-indexed labels, two new navigation tests, mock the two list routes in `beforeEach`); add the four permission strings to `e2e/test-utils.ts` and `e2e/helpers/auth.ts`; new `e2e/pull-requests.e2e.ts` and `e2e/releases.e2e.ts`; `e2e/layout-fixtures.ts` `setupCommonRoutes` branches for `/pull-requests`, `/releases`, `/releases/options` (the catch-all `{}` would fail zod); `e2e/screenshots.e2e.ts` and `e2e/workspace-layout.e2e.ts` entries for both pages (new baseline screenshots on the first PR; say so in the PR).

## Delivery

- Execute through the consolidation skill: backend and frontend as parallel worktree architects against the settled contract above, then consolidate, review, verify. Backend step 1 (the client move) lands first as its own commit so both sides build on it.
- Stack three PRs against `main` so CodeRabbit reviews each (it refuses oversized PRs): (1) `refactor(github): lift the GitHub client into services` (pure move + origin + credentials), (2) `feat(github): pull request and release services, routes and permissions`, (3) `feat(console): Pull Requests and Releases pages`. Conventional commits, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Wait for green CI before merging any of them.
- Format/lint before every commit: `cargo fmt`, clippy `-D warnings`, Biome (`bun run check`).

## Verification

Backend (from `runner/`, Postgres + Valkey running, `DATABASE_URL` / `TEST_DATABASE_URL` → a migrated `zone_test`, `REDIS_URL`):

```bash
cargo fmt --all -- --check && cargo clippy --workspace --exclude zone_desktop --all-targets --all-features -- -D warnings
```

```bash
(cd zone_server && sqlx migrate run) && cargo nextest run --workspace --exclude zone_desktop --all-targets --no-fail-fast --test-threads 8 --no-default-features --features zone_context/test-utils
```

Focused: `cargo nextest run -p zone_server --no-default-features --features zone_context/test-utils -E 'test(/github|pull_request|release|snapshot|origin|credential|permission|migration|config/)'`. Enterprise path: set `GITHUB_API_URL=https://ghe.example/api/v3` against the fake and assert `/api/v3/repos/...` and `/api/graphql`.

Frontend (from the repo root then `manager/frontend`; this worktree has no `node_modules`):

```bash
bun install && bun run build:ui
```

```bash
cd manager/frontend && bun run typecheck && bun run lint && bun run format:check && bunx biome check --linter-enabled=false --formatter-enabled=false src && bun run test
```

```bash
cd manager/frontend && bun run test:e2e -- --project=chromium e2e/navigation.e2e.ts e2e/pull-requests.e2e.ts e2e/releases.e2e.ts e2e/workspace-layout.e2e.ts e2e/mobile.e2e.ts
```

Manual, against `make dev` (backend :8000, console :3001) with a workspace holding GitHub sources: sidebar order and active states (expanded, collapsed, mobile drawer); PR groups/counts match GitHub; tabs and selects filter; detail facts; Network tab shows `/diff` only when the Diff tab opens, `/log` only on "Show logs"; `?id=` deep link; merge a throwaway PR (409 on stale head, success toast, releases refetch); polling only while visible; releases day groups, chips updating while a run is active, expand PRs; wizard publishes to a sandbox repo, 409 shows "Reload options"; a viewer sees no Merge / New release and `/unauthorized` without read. Take screenshots of both pages (list + detail, wizard open) in light and dark at 1440×1000 and 390×844 and check them before reporting done; no horizontal scroll. Per CONTRIBUTING.md, finish with `make live-verify`.

## Follow-ups (not in this change)

- Slice 2, agent actions on zone's task machinery: join `tasks.pr_url` → task/status/auto badge and `/tasks?id=` link; `POST .../pull-requests/{source_id}/{number}/fix { instructions }` creates/adopts a task on the project linked to the repository and queues a run (needs `task_runs.instructions` + `create_run` body, `projects.find_by_repository`, `tasks.adopt_pull_request`); merge enqueues `workers::pr::repair_conflicts_for_task` on a conflicted task-linked PR (202); "In progress" also covers a linked task's active run; `review_signals` multi-select on the Sources form. Fix is a small row button (puller's latest WIP), not a panel.
- Slice 3: adopt foreign PRs into tasks (source ↔ project token bridge); Postgres snapshot table + housekeeping sweep if rate limits bite; `users.github_login` for a default "Mine" filter; favourites/hidden prefs; syntax highlighting in the diff; replace the literal admin permission list in `db/users.rs` with a `permissions` query; type the client's `String` errors.

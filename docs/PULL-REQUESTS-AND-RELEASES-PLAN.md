# Pull Requests and Releases

Two console pages, Pull Requests and Releases, ported from puller (`~/Local/puller`, a single-user `gh` dashboard). Zone's backend serves them from the workspace's stored GitHub sources. They are built with zone's design system and gated by zone permissions. Puller's local coding-agent launchers are out of scope (see Follow-ups).

The work ships as three stacked PRs:

1. `refactor(github): lift the GitHub client into services`, merged as #254.
2. `feat(github): pull request and release services, routes and permissions`, the backend this document describes as built.
3. `feat(console): Pull Requests and Releases pages`, the frontend.

The new GitHub API calls come from abnegate-vcs 0.2.2 on crates.io.

## Decisions

| Question | Decision |
|---|---|
| What the PR page lists | Every open PR across the workspace's active `github` sources. Repository and Author filters run client-side. There is no "Mine" view, because zone has no GitHub identity per console user. |
| Readiness | `services/github/readiness.rs::assess`, unchanged, run against each source's `review_signals`. Drafts are never ready, and a PR with no checks reads as `checks_absent`. |
| Buckets | `ready` if the assessment is ready. Otherwise `in_progress` if checks are pending. Otherwise `not_ready`. Sorted by bucket, then `updated_at` descending. |
| Releases listed | Non-draft releases published within the window: 7 days by default, 1..90 allowed. Each source is read once at the 90-day maximum and cached under its source id; the requested window is cut from that read in memory. PR links are parsed from the notes. Drafts and releases with a null `published_at` are dropped before `release_identity`. |
| Caching | Per-source in-memory snapshots on `AppState`. Each has a TTL, a refresh floor, single-flight, stale-on-error, a rate-limit backoff and incremental re-assessment. `refresh=1` skips only the TTL: re-assessment stays incremental. A snapshot is stamped when its fetch settles. A key whose first fetch fails is dropped unless it is backing off, and a key nobody reads for an hour is swept on the next read. There is no table. |
| Rate limits | A spent quota (403/429 with `x-ratelimit-remaining: 0`) or a secondary limit (429) stops the source's assessment fan-out and backs the source off until GitHub's reset, or 5 min when GitHub names none. Every GitHub fan-out holds a permit from its credential's semaphore (8 per installation, or per source for a stored token), and diffs share a server-wide semaphore of 4. |
| Permissions | `pull_requests:read`, `pull_requests:update` (merge), `releases:read` and `releases:create`. Admin and user roles get all four; viewers get the two reads. The server enforces the writes: merge needs `pull_requests:update`, preview and create need `releases:create`, each on top of `WorkspaceWriter`. Tenancy is decided per request by `WorkspaceMember` (reads) and `WorkspaceWriter` (writes). |
| Merge | Re-assess the single PR. Refuse unless it is open, unmerged, ready, and the expected head matches. Then `PullRequestService::merge`: REST first, falling back to the GraphQL admin merge only when `ZONE_GITHUB_ADMIN_MERGE` is on (default off) **and** the caller is a workspace admin or owner. The method is `merge` (default), `squash` or `rebase`. A squash's message lists every commit's message and ends with their `Co-authored-by` trailers, deduplicated. The merge and its audit run in a spawned task, so a caller that hangs up cannot cut them off. |
| Create release | Preview first, which generates the notes and a digest. Create then does, in order: re-preview, require the digest to match, write the tag object, the ref and the draft release, publish, and read the release back. When the publish answer is lost, the release is read back before anything is reported. It never deletes. A partial failure returns 409 `release_manual_reconciliation_required`. Only one create runs per source at a time, guarded by an in-process set: zone runs a single manager instance, so the guard is per process. The create and its audit run in a spawned task. The tagger is `Zone <zone@users.noreply.github.com>`, and the marker is `<!-- zone-release:<uuid> -->`. |
| Tags | `^(?:(.+)/)?v?X.Y.Z(-pre)?$`, and a valid git ref name. Tags are grouped into lines by their `prefix/`. The options' `latest_tag`, `next_tag` and `previous_tags` describe the dominant line (the one with the most tags; the unprefixed line wins a tie), and `lines` describes every line. Preview and create refuse a tag that is not newer than its own line's latest stable tag, and a tag that already has a release, drafts included. `prerelease` is derived from the tag (`-pre` means prerelease) and a create that disagrees is a 400. A release is made the latest only when its tag is stable and newer than every stable tag in the repository. All tag pages are read. |
| Nav | Chats, Projects, Tasks, **Pull Requests**, **Releases**, Models, Wiki, Settings. |

Names to avoid:

- `crate::pull`, `PullRegistry`, `/ws/pull` and the frontend `PullProvider` are Ollama model pulls.
- `workers::pr` publishes PRs from runs.
- `services::github::release` is the release-pipeline domain logic, which is separate from the `services::github::releases` service.

## API contract

Every route is behind `require_auth` and returns JSON. Errors are `{ success: false, error }`, and 409 refusals add `code`. A malformed path, query or JSON body is a 400 in the same shape. A permission refusal is a 403 `{ error }` naming the permission. Paths carry only UUIDs, numbers and SHAs, so metrics labels fold; tags travel in bodies and queries.

| Method | Path | Guard | Response |
|---|---|---|---|
| GET | `/api/workspaces/{workspace_id}/pull-requests?refresh=1` | member | `{ generated_at, stale, partial, ttl_secs, refresh_floor_secs, counts: {ready, in_progress, not_ready, total}, sources: [SourceStatus], pull_requests: [PullRequest], warnings }` |
| GET | `…/pull-requests/{source_id}/{number}` | member | `{ pull_request, threads: [Thread], comments: [BotComment] }` |
| GET | `…/pull-requests/{source_id}/{number}/diff?head=<sha>` | member | `{ diff: Diff }` |
| GET | `…/pull-requests/{source_id}/{number}/commits?head=` | member | `{ commits: { number, head, base, complete, commits: [Commit] } }` |
| GET | `…/pull-requests/{source_id}/{number}/commits/{sha}?head=` | member | `{ diff: Diff }` |
| GET | `…/pull-requests/{source_id}/{number}/jobs/{job_id}/log?head=` | member | `{ log: { job_id, run_id, head, text, truncated, fetched_at } }` |
| POST | `…/pull-requests/{source_id}/{number}/merge` `{ expected_head, method? }` | writer + `pull_requests:update` | `{ merged: { number, sha, admin, url } }` |
| GET | `/api/workspaces/{workspace_id}/releases?days=7&refresh=1` | member | `{ generated_at, stale, partial, window_days, ttl_secs, sources: [SourceStatus], releases: [Release], warnings }` |
| GET | `…/releases/options?refresh=1` | member | `{ generated_at, sources: [SourceOptions], warnings }` |
| GET | `…/releases/{source_id}/{release_id}/pipeline?refresh=1` | member | `{ pipeline, state, succeeded, complete, blockers }` |
| POST | `…/releases/preview` `{ source_id, tag, expected_latest_tag }` | writer + `releases:create` | `{ preview: Preview }` |
| POST | `…/releases` `{ source_id, tag, expected_latest_tag, prerelease, digest }` | writer + `releases:create` | 201 `{ release: { source_id, id, tag, name, url, published_at } }` |

`expected_latest_tag` is the latest stable tag of the proposed tag's own line. The pipeline route answers only for a release id in the source's current release listing (read from its cache, or fetched when absent); any other id is a 404 without asking GitHub.

409 codes:

- Head-keyed reads (diff, commits, commit diff, job log): `head_moved` with `current_head`.
- Merge: `head_moved` (with `current_head`, re-fetched after GitHub's head check fails), `not_ready` (with `blockers`), `conflict`, `protected`, `merged` (the PR was already merged), `closed` (the PR is closed). `head_moved`, `conflict`, `merged` and `closed` also drop the source's cached listing.
- Preview and create: `latest_tag_moved`, `tag_not_newer { tag, latest_tag }`, `tag_exists { tag, tag_object? }`, `release_exists { tag, release_id }`.
- Create: `preview_stale`, `release_in_progress`, and `release_manual_reconciliation_required { tag, tag_object, reference_created: bool | null, release_id: number | null, release_exists, marker }`. `reference_created` is null when whether the tag's ref points at the new tag object could not be read back. `release_exists` is true when GitHub refused the draft because the tag already had a release. `marker` is the uuid in the `<!-- zone-release:<uuid> -->` marker written into the tag message and the release body.

An invalid tag, and a create whose `prerelease` disagrees with its tag, are 400s. GitHub API errors, rate limits, refused or unresolvable credentials, 5xx refusals and transport failures are 502 (`ServerError::BadGateway`); only an unreadable source configuration is a 409 without a code. A merge or create whose task stopped before reporting back is a 500 saying the outcome is unknown.

Row shapes are snake_case. The existing `Serialize` output is reused, so `PipelineState` stays kebab-case.

- `PullRequest` has these fields:
  - Identity: `source_id, repository, repository_url, number, title, url, author, updated_at, draft, head, head_ref, base_ref`.
  - Readiness: `readiness, blockers, checks, check_details: [{kind, id, name, state, url, updated_at}], reviews, review_current, unresolved_threads, threads_complete, comments_complete, assessed_at, rank`.
  - `check_details[].id` is nullable. It is the Actions job id for a `check_run` whose link is that job's page at the configured web origin, and null for every other row, so a non-null id is always a job whose log can be read.
  - `reviews[].url` is an absolute web link or null.
  - In the detail route, `threads_complete` says whether the `threads` list it returns was read in full: the verdict and the list come from one read.
- `SourceStatus`: `source_id, name, repository, repository_url, freshness ('cached'|'refreshed'|'floored'|'stale'), generated_at, warning`.
- `Thread`: `id, path, line, outdated, url, comments: [{id, author, body, url, created_at}]`. `url` is the first comment's URL.
- `BotComment`: `reviewer, body, url, created_at`.
- `Diff`: `number, head, base, complete, warning, files: [{path, previous_path, status, additions, deletions, changes, binary, truncated, blob_url, hunks: [{header, old_start, old_lines, new_start, new_lines, lines: [{kind, content, old_line, new_line}]}]}]`.
- `Commit`: `sha, message, author_name, author_login, authored_at, url`.
- `Release`: `source_id, repository, repository_url, id, tag, name, url, published_at, prerelease, pulls: [{number, url, title}], pipeline, state, succeeded, complete, blockers`.
- `SourceOptions`: `source_id, repository, repository_url, default_branch, latest_tag, next_tag, previous_tags, lines: [{prefix, latest_tag, next_tag, previous_tags}]`. `prefix` is null for the unprefixed line.
- `Preview`: `source_id, repository, tag, name, body, base_tag, target, prerelease, pulls, digest`.

Polling contract for the console:

- The PR list refreshes every 30 s while visible and on focus. Refresh sends `refresh=1` and is disabled inside `refresh_floor_secs`. After a 502 it backs off 10/20/40/80/120 s.
- Releases refresh every 120 s. A release's `pipeline` is polled every 15 s only while the release is younger than 30 min and its lookup is pending or a run is queued or running.
- The server invalidates the source's snapshots after a merge or a create.

## Backend as built (`runner/zone_server`)

### Module map

```
src/services/github/
  mod.rs               module declarations only, no re-exports
  client.rs            Github over abnegate_vcs::PullRequestService and the configured web origin; typed evidence reads;
                       assess(row, detail, Depth) and assess_with(.., threads) -> Result<Assessed, Fault>
    review.rs, tests.rs
  evidence.rs          neutral types client and pull_requests share: Assessed, CheckDetail, CheckKind, Depth (Listing | Full)
  fault.rs             Fault { RateLimited { retry_after, exhausted }, Unauthenticated, Missing, Upstream }: how a GitHub
                       read failed, used by the cache and both services; rendered as text only at the edge
  link.rs              absolute / linked: only http(s) URLs with a host are links
  throttle.rs          Throttle: a semaphore per credential (8) and one for diffs (4); admitted
  authentication.rs    source token resolution, minting again once on a refused token (+ failure.rs);
                       credentialed<T, E> for typed errors, authenticated for the agent's strings
  configuration.rs     Configuration { owner, repository, branch, path, token, review_signals }
  connection.rs        ConnectedSource; active_source / connect / connected_sources
  sources.rs           Sources { connected, invalid }: every active GitHub source, split by whether its configuration parses
  unavailable.rs       Unavailable { Unreadable, Missing, NotGithub }
  unconnected.rs       Unconnected { Unavailable, InvalidConfiguration }
  refusal.rs           Refusal: what GitHub said in refusing a request
  readiness.rs         assessment evidence, ReviewSignals, assess
  release.rs           ReleaseIdentity, ReleasePipeline, RunEvidence, Lookup, merge
  cache.rs             Cache<K, T>: TTL, refresh floor, single-flight, stale-on-error, rate-limit backoff (GitHub's reset,
                       else RATE_LIMIT_BACKOFF, 5 min), stamped at settle, failed keys dropped, IDLE_EXPIRY (1 h) sweep
    decision.rs, entry.rs, slot.rs, tests.rs
  snapshot.rs          Snapshot<T> { value, generated_at, warning }
  freshness.rs         Freshness { Cached, Refreshed, Floored, Stale }
  source_status.rs     SourceStatus
  snapshots.rs         Snapshots { pull_requests, releases (by source), options, pipelines, throttle } plus the
                       per-source releasing set; invalidate, try_release
  releasing_guard.rs   ReleasingGuard: one create per source in this process, released on drop
  diff.rs              unified-diff parser into files and hunks; PER_FILE_DIFF fallback
    entry, file, hunk, hunks, line, line_kind, open_hunk, patch, path, revision, section, status, tests
  pull_requests.rs     list / detail / diff / commits / commit_diff / job_log / merge; bucket, refresh_plan, ranked
    bot_comment, commit, commits, counts, detail, error, listing, log, merged, method, overview, pull_request,
    readiness, refusal, repository, squash (the squash message), thread, thread_comment
    reading.rs         impl Github: open_pulls, pull_commits, pull_diff, commit_diff_excerpt, job_log_tail;
                       unresolved, budgeted (the per-file fallback's patch budget)
  releases.rs          recent / pipeline / options / preview / create
    canonical (digest), catalogue (tags grouped into lines), created, failure, included_pull, line, listing (since),
    notes (pulls_in_notes), offered, placement (whose ref the tag is), preview, proposal, recent, reconciliation,
    refusal, release, source_options, tag (Tag with prefix, next tag), tag_line, tests
    reading.rs         impl Github: releases_since, release_pipelines_for, recent_listing, observed_pipeline, source_options,
                       tag_names, release_tagged, tag_held, tag_reference, branch_head, release_notes, write_tag,
                       write_reference, write_draft_release, publish_draft, release_by_id
  testing.rs           #[cfg(test)] wiremock fixtures (+ testing/connected.rs)
src/auth/github_guard.rs      PullRequestMerger, ReleaseCreator: WorkspaceWriter plus the write's permission
src/routes/refresh_query.rs   RefreshQuery, shared by both pages
src/routes/pull_requests.rs   7 handlers; pull_requests/{head_query, merge_request, merging (the spawned merge and its
                              audit), listing_response, diff_response, commits_response, log_response, merged_response,
                              refusal_response}
src/routes/releases.rs        5 handlers; releases/{window_query, preview_request, create_request, creating (the spawned
                              create and its audit), listing_response, options_response, pipeline_response,
                              preview_response, created_response, refusal_response, refusal_detail, tag_detail,
                              reconciliation_detail, rejection}
migrations/0010_pull_request_and_release_permissions.sql
tests/integration/pull_requests.rs, tests/integration/releases.rs   wiremock GitHub, own Postgres
```

`Github` has four `impl` blocks: `client.rs`, `agent/integrations.rs` (the LLM tool JSON), `pull_requests/reading.rs` and `releases/reading.rs`. Method names must stay unique across all four.

The tool path in `agent/integrations.rs` calls `assess` with `Depth::Full`, which keeps the changed-files walk, and its JSON is unchanged. The console list calls `assess` with `Depth::Listing`, which skips that walk, so risk reads as assumed. The client's reads return `Fault`; the agent's string-returning paths take its text through `From<Fault> for String`, so their words are unchanged.

### abnegate-vcs 0.2.2 calls

- Pull requests:
  - `list_pull_requests` and `fetch_pull`.
  - `collect_check_runs`, `collect_statuses` and `collect_workflow_runs`.
  - `collect_review_threads`, `fetch_review_thread_counts`, `fetch_issue` and `list_comments`.
  - `fetch_diff`, with `fetch_files` as the 406 fallback.
  - `list_commits` and `fetch_commit_diff`.
  - `fetch_workflow_job` and `fetch_job_log_tail`.
  - `merge`.
- Releases:
  - `list_releases` and `fetch_release_by_id`.
  - `list_tags`, `tag_exists` and `fetch_tag_reference`.
  - `get_default_branch` and `resolve_commit`.
  - `generate_release_notes`.
  - `create_tag`, `create_tag_reference`, `create_release` and `publish_release`.

### State, config, permissions, audit

- **State:** `AppState.github_snapshots: Snapshots`, built from `config.github` in every constructor. The accessor is `github_snapshots()`. `AppState::github()` is still the GitHub App provider.
- **Config:** `GithubConfig { pull_request_ttl: Duration, release_ttl: Duration, release_window_days: u32, open_pull_request_limit: u32, admin_merge: bool }`, read from these variables:

  | Variable | Default | Range |
  |---|---|---|
  | `ZONE_GITHUB_PULL_REQUEST_TTL_SECS` | 45 | 10..3600 |
  | `ZONE_GITHUB_RELEASE_TTL_SECS` | 120 | 10..3600 |
  | `ZONE_GITHUB_RELEASE_WINDOW_DAYS` | 7 | 1..90 |
  | `ZONE_GITHUB_OPEN_PULL_REQUEST_LIMIT` | 50 | 1..200 |
  | `ZONE_GITHUB_ADMIN_MERGE` | false | |

  They are documented in `docs/CONFIGURATION.md`. The service constants are pipeline TTL 15 s, options TTL 300 s, refresh floor 10 s, diff budget 4 MiB (also the per-file fallback's patch budget), diff permits 4, credential permits 8, log tail 1 MiB, failed-check re-assessment after 120 s, any kept assessment re-assessed after 5 min, and idle cache keys swept after 1 h.
- **Origin:** `GITHUB_API_URL` was already validated in `config.rs`. The crate handles GitHub Enterprise `/api/v3` and `/api/graphql`. The web origin, `GITHUB_WEB_URL`, is held by each `Github` client: an Actions job link and a release-triggered run link are accepted only at that origin, so GitHub Enterprise job logs and release pipelines are read.
- **Permissions:** `0010_pull_request_and_release_permissions.sql` seeds the four permissions and the role grants. Both use `ON CONFLICT DO NOTHING`, with no `BEGIN;`/`COMMIT;`. The four names are also in the hard-coded admin list in `db/users.rs::get_user_with_permissions`. `auth::Permission` gains `PullRequestsUpdate` and `ReleasesCreate`, which `auth/github_guard.rs` requires from the caller's claims.
- **Audit:** `actions::PULL_REQUEST_MERGED` and `actions::RELEASE_CREATED` on `resources::PULL_REQUEST` and `resources::RELEASE`, with the source id as `resource_id`, written through `routes::common::audit` after success inside the spawned task. The merge records `source_id, number, head, method, sha, admin, url`; the release records `source_id, release_id, tag, url, prerelease`.
- **Errors:** the existing `ServerError::BadGateway` serves as the 502. `error.rs` was not changed.

### Deviations from the original plan

Pull requests:

- `Diff.base` is the base **branch name**, not a SHA. Deleted-file `blob_url`s point at that branch.
- When GitHub refuses a commit diff with 406, the response is an **empty, incomplete** `Diff` carrying `UNRENDERED_COMMIT`. There is no per-file fallback for commits.
- `check_details[].id` is null except for an Actions `check_run`, where it is the job id.
- The detail route returns its `pull_request` with `rank: 0`. Rank is only meaningful in the list.
- The job log is read from its end with `fetch_job_log_tail`. The response returns the log's **last** 1 MiB, starting at a whole character, and `truncated` says whether earlier bytes were dropped. github.com's log store ignores a suffix range, so when it answers with the whole log and serves byte ranges, the end is asked for again from 1 MiB before it. A long log's failure is never cut off.
- Commit membership (the commit diff route) and the squash message read every page of the PR's commits. GitHub lists at most 250 commits for a PR, so a commit past that is refused as foreign.

Releases:

- Preview requires write access (`WorkspaceWriter`) and `releases:create`, like create. Viewers cannot preview.
- Tags must match `[prefix/]v?X.Y.Z[-pre]`. Anything else is a 400.
- The listing's `generated_at` is the **oldest** per-source snapshot time.
- A stable tag is made the repository's latest release only when it is newer than every stable tag already there; a prerelease never is.

## Frontend (PR 3, `manager/frontend`)

`features/pull-requests/` and `features/releases/` hold `schemas.ts` (zod, the single adapt point to the contract above), `types.ts`, `api` modules in the `api/tasks.ts` shape, hooks, components and pages. `client.ts` does not register them. Each page gets a lazy loader and a signed-in route in `src/routes.ts`, plus a route in `App.tsx` behind `ProtectedRoute` on its read permission. Merge and New release are gated on the write permission **and** `useWorkspace().writable`.

- **Pull Requests:** master-detail on the shared resizable `SubNav` (storage key `pull-requests-pane`). The tabs are All / Ready / In progress / Not ready. It has Repository and Author filters and a deep link `?id=<source_id>:<number>`. The detail tabs are Blockers, Diff and Commits. Artifact queries are keyed on the head SHA.
- **Releases:** a single scroll body grouped by day, with pipeline chips, an expandable list of included PRs, and a two-step create wizard (Target, Review). A 409 there offers "Reload options".
- **Styling:** `--ui-*` tokens only. Shared detail classes come from `ProjectsPage.css`. Each page has a `describe` block in `pages.design.test.ts`.
- **Navigation:** the sidebar has 8 items in the order above. `Sidebar.test.tsx` and `e2e/navigation.e2e.ts` (`toHaveCount(8)`) pin it. Empty states link to `/settings/workspace/sources`.
- **Contract tests:** `schemas.contract.test.ts` in each feature pins the zod schemas to the Rust wire structs through `src/test/rust.ts`.

## Verification

Backend, from `runner/`, against a migrated Postgres with both `DATABASE_URL` and `TEST_DATABASE_URL` set:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude zone_desktop --all-targets --all-features -- -D warnings
cargo nextest run --workspace --exclude zone_desktop --all-targets --no-fail-fast --no-default-features --features zone_context/test-utils
cargo deny --locked check --warn unmaintained --warn notice
```

Frontend, from `manager/frontend`:

```bash
bun run typecheck && bun run lint && bun run format:check && bun run check && bun run test
```

Run the e2e specs `navigation`, `pull-requests`, `releases`, `workspace-layout` and `mobile` with `bun run test:e2e`.

Manual pass against `make dev`, with a workspace that holds GitHub sources:

- PR groups and counts match GitHub.
- `/diff` and `/log` are fetched only on demand.
- Merging a throwaway PR works, and a stale head gets a 409.
- A release is published to a sandbox repo.
- A viewer sees no Merge or New release.

Take screenshots in light and dark at 1440×1000 and 390×844. Finish with `make live-verify`.

## Follow-ups

- Agent actions on zone's task machinery: link `tasks.pr_url` to a task badge, add a Fix action that queues a run, repair conflicts on merge, and let "In progress" cover a linked task's active run.
- `users.github_login` for a "Mine" filter, favourites and hidden prefs, and diff syntax highlighting.
- A Postgres snapshot table if rate limits bite. Replace the literal admin permission list in `db/users.rs` with a query.

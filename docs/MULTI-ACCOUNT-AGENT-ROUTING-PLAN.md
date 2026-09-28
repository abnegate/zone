# Multi-account agent sign-in, usage routing, session resume, and in-chat handover

## Context

PR #98 (merged to `main` as `19066914`) lets an organization sign in to **one** Claude Code account and **one** Codex account and runs every chat turn and task attempt on that sign-in. Zone stores the sign-in in `agent_logins` with `UNIQUE (organization_id, agent)`, spawns a fresh CLI process per turn with the **whole transcript on stdin** (no `--resume`, so every turn pays full input price again; only the CLI's system prompt and tool definitions hit the prompt cache), and reduces a usage-limit refusal to a failure string: the chat shows an error and stops; a task backs off and retries on the same account.

This change lets an organization sign in to **several** accounts per agent, starts each chat or task run on the account with the most remaining usage and **stays on it** until that account runs out, keeps a **CLI session per chat** so later turns send only the new message, and when the account a turn is on hits its limit, hands the turn over to another account **in the same chat** with a visible notice and carries on (copying the CLI session file into the other account's home so context is not re-sent; falling back to a transcript replay when it must). Usage comes from the `aiusg` crate, published to crates.io and pulled in as a library.

Branch `multi-account-signin-routing-1be6cc` (worktree `sharp-bell-b82b40`) is at `abebd560`, 165 commits behind `origin/main`, with no commits of its own. Step 0 is bringing it onto `origin/main` (`sync_with_base_branch`).

### Decisions (confirmed with Jake)

- **Sticky sessions.** Routing happens when a chat's first CLI turn or a task run starts, not per turn. A chat stays on its account until that account is exhausted (a limit event, or `exhausted_until` set by another chat) or signed out; only then does it switch. Headroom alone never moves a running chat.
- **Cross-agent handover is allowed.** The workspace/org provider setting is a preference: the router ranks the configured agent's logins first, then every other signed-in agent's logins by headroom. A chat that lands on Codex stays there until Codex runs out. Cross-agent switches replay the transcript (a Claude session cannot be resumed by Codex) and re-pick the model from the new agent's catalog.
- **CLI session resume is in scope.** First turn `claude -p --session-id <uuid>` / `codex exec`; later turns `claude -p --resume <id>` / `codex exec resume <id> -` with only the new message. A same-agent handover copies the session file between login homes and resumes. Needs a spike (below) to confirm both CLIs resume a file created under another config dir; the transcript replay stays as the fallback path.
- **aiusg goes on crates.io** (name is free). Zone depends on `aiusg = "=0.5.0"` by version, not git.
- **Grok is a follow-up PR.** Everything is provider-agnostic; aiusg's `grok::{fetch, discover_in}` land now.
- **Default Claude sign-in scope becomes `user:inference user:profile`** (usage and profile endpoints need it; email label). Verify the grant's lifetime at implementation; refresh-token renewal already exists. Pre-existing sign-ins read as usage unknown until re-signed in.
- **Accounts stay organization-level.** Tasks route at run start and reroute on limits; they keep transcript replay per attempt (session resume for task attempts is a follow-up).
- **magents is not used** for the handover: its `handoff` targets live TUI sessions over a socket/tmux/headless resume; Zone's children are headless one-shot processes it spawns itself. Copying a session file between per-login homes is the same trick magents' `Homes` isolation relies on, done by Zone directly.

### Dependency facts (verified by trial resolves in the scratchpad)

| Fact | Consequence |
|---|---|
| aiusg is not on crates.io yet; tags `0.1.0`..`0.4.1` | P0 publishes `0.5.0`; Zone depends by version |
| aiusg `reqwest = "0.13.5"` vs Zone `reqwest = "=0.13.4"` | Bump Zone's workspace pin to `=0.13.5` (latest) |
| aiusg `rusqlite 0.40.2` → `libsqlite3-sys 0.38.2`; Zone's lock has `sqlx-sqlite 0.9.0` → `libsqlite3-sys <0.38`; both `links = "sqlite3"` → **Cargo refuses to resolve** (reproduced even with sqlx's sqlite feature off) | aiusg puts `rusqlite` behind a non-default feature; reproduced that an inactive optional dep never enters the consumer's lock |
| aiusg has no `[features]`; `crossterm`, `webbrowser`, `keyring`, `clap`, rmcp server are unconditional | Same aiusg PR gates them; Zone uses `default-features = false` |
| `keyring` on Linux pulls `libdbus-sys` | Gating keyring keeps the manager Dockerfile unchanged |
| CLIs honour `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `GROK_HOME`; sessions live at `<config>/projects/<sanitized cwd>/<id>.jsonl` and `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl`; `claude` has `--session-id`, `--resume`, `--append-system-prompt`; `codex exec resume [SESSION_ID] [PROMPT]` reads `-` from stdin and prints `thread.started.thread_id` | Per-login homes are env vars; the shared work cwd makes the session path identical in every login's home |

## How it fits together

1. **Sign-in** creates a row per account. Claude: sealed tokens in the row, label/`account` = email from `aiusg::provider::claude::profile`. Codex: `auth.json` in the login's own `CODEX_HOME`, `account` = `account_id` from the id_token. Re-signing in the same account replaces its row.
2. **Homes.** `<state>/<org>/<agent>/logins/<login-id>` is one login's `CLAUDE_CONFIG_DIR`/`CODEX_HOME`; `<state>/<org>/<agent>/work` stays the shared cwd for every login's turns. A pre-049 `<state>/<org>/codex/auth.json` is adopted into the first login's home lazily.
3. **Routing at session start.** `backend::for_settings` asks the router for candidates across the org's logins (configured agent first), refreshes usage snapshots older than `ZONE_AGENT_USAGE_TTL_SECONDS` (aiusg `fetch`, parallel, 5 s timeout, stale fallback), ranks by aiusg's `Usability` (most headroom first; unknown after known; exhausted/signed-out never), returns the backend plus which login and agent it runs under. The chat records the login; later turns reuse it unless it is exhausted or gone.
4. **Sessions.** The chat records the CLI session id, its agent, and the last `chat_entries` position the session has seen. A resumed turn sends only entries after that position (the new user message plus any per-turn system notes); a first turn sends the full rendered transcript and pins the id (`--session-id` for Claude; `thread.started.thread_id` captured for Codex). A resume the CLI refuses (`No conversation found`, exit before init) falls back to a full replay under a fresh id.
5. **Structured limit signal.** Parsers turn a refusal into `AgentEvent::Limited(Limit { message, resets_at, credits, window })` and an allowed `rate_limit_event` into `AgentEvent::Window` (free usage updates), flowing through `ProviderError::Limited` → `LlmError::Limited` → the server's `AgentEvent::Limited`.
6. **Chat handover.** In `handle_chat_generation`'s outer loop (the seam that already reruns a round after a wait settles): mark the login exhausted until its reset, pick the next candidate (excluding those tried this turn), carry the session (same agent: copy the session file into the new home and resume with a continue instruction; other agent or no session: replay the transcript with the partial text and the continue instruction under a fresh session), swap the client's backend (and model if the agent changed) keeping the same MCP lease and assistant message id, publish `Status` + `Handover`, record the switch in the assistant message's metadata, rerun. No candidate left → today's failure path naming the earliest reset.
7. **Task handover.** A `Limited` fault reroutes at once, costing no attempt and no backoff; backoff only when no login has headroom.
8. **Console.** The sign-in panel becomes an account list with usage bars, per-account sign-out, "Add another account"; the chat renders handover dividers inside the answer.

## Shared contracts (parallel agents code against these verbatim)

### zone_core (`runner/zone_core/src/llm/provider/`)

`window.rs` (one type, mirrors aiusg's `Window`, also the stored snapshot record):
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window { pub name: String, pub used_percent: Option<f64>, pub used: Option<u64>, pub limit: Option<u64>, pub resets_at: Option<DateTime<Utc>> }
```
`limit.rs`:
```rust
pub const LIMIT_WORDINGS: [&str; 5] = ["hit your", "session limit", "usage limit", "weekly limit", "rate limit"];
#[derive(Debug, Clone, PartialEq)]
pub struct Limit { pub message: String, pub resets_at: Option<DateTime<Utc>>, pub credits: bool, pub window: Option<Window> }
```
`session.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session { pub id: String, pub resume: bool }
```
`event.rs`: `AgentEvent` gains `Window(Window)`, `Limited(Limit)` (terminal), `Session(String)` (the id the CLI announced: Claude `system/init.session_id`, Codex `thread.started.thread_id`); replace the doc comment arguing against a throttling variant. `settings.rs`: `CliSettings.session: Option<Session>`; `arguments_with` adds `--session-id <id>` (new) or `--resume <id>` (resume) for Claude, and `exec … -` vs `exec … resume <id> -` for Codex. `error.rs`: `ProviderError::Limited { provider, limit }` (recoverable; redacted). `client.rs`: `LlmError::Limited(Limit)`; `LlmClient::with_backend(self, LlmBackend) -> Self` (drops the toolset; callers re-attach). `types.rs`: `ChatStreamChunk` gains `#[serde(skip)] pub window: Option<Window>` and `#[serde(skip)] pub session: Option<String>`; `Usage` gains `Copy` and `plus()`. Re-export `Limit`, `Window`, `Session`, `LIMIT_WORDINGS` from `llm/mod.rs`.

### zone_server

`services/login/identity.rs`: `pub struct LoginIdentity { pub id: Uuid, pub agent: AgentKind, pub label: String }`.

`services/backend/resolved.rs`: `pub struct Resolved { pub backend: LlmBackend, pub login: Option<LoginIdentity> }`.

`services/backend.rs`:
```rust
pub async fn for_workspace(state: &AppState, workspace: Uuid) -> Result<Resolved, Error>;
pub async fn for_settings(state: &AppState, organization: Uuid, settings: &EffectiveAiSettings, exclude: &[Uuid], sticky: Option<Uuid>) -> Result<Resolved, Error>;
pub fn on_login(config: &Config, organization: Uuid, chosen: &Chosen, session: Option<Session>) -> Result<Resolved, Error>;
pub enum Error { SignedOut { agent }, Limited { agent, resets_at: Option<DateTime<Utc>> }, Renewal { .. }, Home { .. }, Database { .. } }
```
`sticky` = the chat's current login: returned as-is when it is signed in and not exhausted; otherwise routing runs. `Limited` displays "Every sign-in of this organization has reached its usage limit; the earliest resets at {when}." (or "; none reports when it resets.").

`services/login/router.rs` (+ `router/chosen.rs`):
```rust
pub async fn pick(state: &AppState, organization: Uuid, preferred: AgentKind, exclude: &[Uuid], sticky: Option<Uuid>) -> Result<Chosen, Error>;  // Error::None | Exhausted { resets_at } | Database
pub async fn mark_limited(state: &AppState, login: Uuid, limit: &Limit);
pub async fn observe(state: &AppState, login: Uuid, window: &Window);
pub async fn settle(state: &AppState, login: Uuid);
pub struct Chosen { pub login: AgentLoginRow, pub agent: AgentKind, pub resolved: Login, pub snapshot: Option<Snapshot> }
```

`services/agent/sessions.rs`:
```rust
pub fn locate(home: &Path, agent: AgentKind, work: &Path, id: &str) -> Option<PathBuf>;   // Claude: projects/<sanitized work>/<id>.jsonl; Codex: sessions/**/rollout-*-<id>.jsonl
pub fn carry(from: &Path, to: &Path, agent: AgentKind, work: &Path, id: &str) -> Result<Carried, Error>;   // copies the file under the same relative path, 0600; Error::Missing when `from` has no file
pub fn sanitized(work: &Path) -> String;   // Claude's project folder name for a cwd ('/' → '-')
```

`config.rs`: `AgentConfig::login_home(&self, organization, agent, login) -> PathBuf`, `create_login_home(..) -> io::Result<PathBuf>` (0700, also creates `work`), free fn `agent_login_home(state, organization, agent, login)`, `usage_ttl: Duration` from `ZONE_AGENT_USAGE_TTL_SECONDS` (integer seconds, default 60, refused otherwise).

Chat columns (migration 049): `chats.agent_login_id UUID NULL REFERENCES agent_logins(id) ON DELETE SET NULL`, `chats.agent_session_id TEXT NULL`, `chats.agent_session_agent TEXT NULL`, `chats.agent_session_entry BIGINT NULL` (last `chat_entries.position` the session has seen). `db/chats.rs`: `session(pool, chat) -> Option<ChatSession { login, id, agent, entry }>`, `set_session(pool, chat, Option<&ChatSession>)`.

Websocket: `ServerMessage::Handover { message_id: Uuid, from: String, to: String, agent: AgentKind, reason: String, resets_at: Option<DateTime<Utc>>, carried: bool, at: usize }` (`"type":"handover"`; `at` = count of Unicode scalar values of the assistant content before the switch; `reason` ∈ `limit | credits | signed_out`; `carried` = the session file moved with the turn). The assistant message's `metadata.handovers` holds the same objects; `metadata.usage` sums tokens across accounts.

Status JSON (`GET /api/organizations/{org}/agents`; shared fixture `runner/zone_server/tests/fixtures/agents.json`, pinned byte-for-byte by a Rust test and read by six console test files):
```json
{"agent":"claude","provider":"claude_code","state":"signed_in","source":"zone","label":"jake@example.com","expires_at":null,"models":["sonnet","opus","haiku"],"pending":null,"error":null,
 "logins":[{"id":"3f2b9c1e-6d4a-4f0b-9c7e-1a2b3c4d5e6f","label":"jake@example.com","plan":"Claude Max","state":"signed_in","expires_at":"2027-09-23T04:00:00Z",
   "usage":{"windows":[{"name":"5h","used_percent":62.0,"used":null,"limit":null,"resets_at":"2026-09-23T06:10:00Z"},{"name":"7d","used_percent":31.0,"used":null,"limit":null,"resets_at":"2026-09-28T04:00:00Z"}],"headroom":38.0,"fetched_at":"2026-09-23T04:00:00Z","exhausted_until":null},
   "last_used_at":"2026-09-23T03:50:00Z"}]}
```
Agent-level `label`/`state` summarise the best login; `usage` is `null` without a snapshot; codex entries carry `"logins": []` when none.

Routes: existing shapes stay; `list`/`get` include `logins`; new `DELETE /api/organizations/{org_id}/agents/{agent}/logins/{login_id}`; `DELETE …/agents/{agent}/login/attempt` also cancels a pending Codex device sign-in; the old `DELETE …/agents/{agent}/login` stays (signs every login out), the console stops calling it.

## Work packages

Ownership is by file. Shared edits: `services/login/mod.rs` (P3 adds `homes`, `identity`; P4 adds `router`, `usage`) and `ws/chat.rs` (P2 adds session plumbing, P6 the handover; P6 starts after P2 merges).

### P0 — aiusg 0.5.0 on crates.io (repo `~/Local/aiusg`, own PR, then release)

Branch from `origin/main` (`7dc783f`). Set `version = "0.5.0"` in the PR.

- `Cargo.toml`: `[[bin]] required-features = ["cli"]`; features `default = ["cli"]`, `cli = ["login","keychain","copilot","cursor","grokbot","dep:clap","dep:crossterm","dep:rmcp","dep:schemars","dep:futures"]`, `login = ["dep:webbrowser"]`, `keychain = ["dep:keyring"]`, `copilot = ["dep:rusqlite"]`, `cursor = ["dep:rusqlite"]`, `grokbot = ["cursor"]`; those deps (incl. the three target-specific `keyring` entries) become `optional = true`. Everything else stays unconditional.
- `cfg` placement: `lib.rs` gates `app`, `cli`, `mcp` on `cli`; `render/mod.rs` gates `table`, `watch`; `oauth.rs` gates `Loopback`, `prompt_open`, `RESPONSE` on `login`; `provider/mod.rs` gates the `copilot`/`cursor`/`grokbot` modules and the *match arms* in `fetch`/`refresh`/`discover` (enum unchanged; `cfg(not)` twins return `Unsupported`); `provider::login` on `login`; per-provider `login`/`AUTHORIZE_URL`/`SCOPES`/`CALLBACK_PORTS` on `login`; keychain bits on `keychain` (`Backend::from_env` returns `File` without it).
- Public API additions: `provider::Unsupported { provider, feature }`, `provider::is_signed_out(&anyhow::Error) -> bool`, `Fetched::into_usage(self, &Account) -> Usage`, `Provider::feature()`, `claude::{Profile, ProfileAccount, ProfileOrganization}` with `label()`/`plan()`, `claude::profile(http, access_token) -> Result<Profile>`, `claude::discover_in(config_dir)`, `codex::discover_in(home)`, `grok::discover_in(home)` (existing `discover()` delegates; missing file → empty, unreadable → error).
- Tests (in-module, `tempfile`): `discover_in_*` for claude/codex/grok (reads, missing file, empty token, unparsable), profile label/plan precedence, `a_provider_left_out_of_the_build_names_the_feature_that_adds_it`, `a_signed_out_error_is_recognised_through_added_context`, `a_fetch_becomes_usage_for_its_account_and_keeps_the_stored_plan_when_none_came`, `optional_providers_name_their_feature`.
- CI: new `library` job (`cargo build/clippy/test --no-default-features`, `--features login`, `! cargo tree --no-default-features -e normal | grep -E "crossterm|webbrowser|rusqlite|keyring|clap|rmcp|schemars"`, and `cargo publish --dry-run`). `release.yml`: after the binaries, `cargo publish --locked` with `CARGO_REGISTRY_TOKEN` (**Jake adds the secret to the repo**, or runs `cargo publish` once locally for 0.5.0).
- README "Use as a library" (`aiusg = { version = "0.5", default-features = false }`, feature table, short example, `AIUSG_DEBUG` note). Release notes in the GitHub Release body.
- Gate: CI green → merge → `gh release create 0.5.0` → `release.yml` green → `cargo search aiusg` shows 0.5.0. Only then P9's Cargo change resolves.

### P1 — zone_core structured limit signal (+ server runner)

Files: `zone_core/src/llm/provider/{limit.rs (new), window.rs (new), event.rs, error.rs, cli.rs, mod.rs, parser/claude.rs, parser/codex.rs}`, `zone_core/src/llm/{client.rs, types.rs, mod.rs}`, `zone_server/src/agent/runner.rs`, `zone_server/src/workers/task/halt.rs` (new), `workers/task.rs` (only `run_task_loop`'s error type + a stub `Fault::limited` classifying as `RateLimited`).

- `parser/claude.rs`: rename the deserialize struct `Limit` → `Info`; parse `resetsAt` (seconds) and `utilization` (**a fraction; ×100**); `Event::Result` gains `api_error_status`. Refusal → `AgentEvent::Limited` (`credits: true` on the `UNFUNDED` paths, exact wording kept); allowed `rate_limit_event` with `utilization` → `Window`; failed `result` with `api_error_status == 429` or `LIMIT_WORDINGS` → `Limited { resets_at: None }`; sign-in failures stay `Failed`.
- `parser/codex.rs`: `turn.failed` matching `LIMIT_WORDINGS` → `Limited`; 401s stay `Failed`.
- `cli.rs`: `Limited` halts the child like `fail()` → `ProviderError::limited` (stderr tail appended); `Window` passes through.
- `client.rs`/`types.rs`/`runner.rs`/`halt.rs` per the contract (`Halt { Failed(String), Limited(Limit) }`, `run_task_loop -> Result<TurnOutcome, Halt>`).
- Tests: `a_refused_request_is_a_limit_with_its_reset_time_and_window` (fixture → `credits: true`, `resets_at == 1790208000`, window 100%), `a_headroom_report_becomes_a_window_and_not_a_failure` (0.43 → 43.0; replaces the invented `72.5` test), `a_rejected_window_keeps_the_wording_the_worker_reads`, `a_failed_result_in_limit_words_is_a_limit_without_a_reset_time`, `a_signed_out_result_is_a_failure_and_not_a_limit`, `an_exhausted_quota_is_a_limit_without_a_reset_time` (codex), `a_limited_turn_ends_the_stream_as_a_limit_error`, `a_window_report_reaches_the_consumer_without_ending_the_turn`, `a_limit_carries_the_end_of_stderr_after_the_agents_words`, `a_limited_agent_stream_ends_in_a_limited_error_not_an_agent_one`, `a_window_rides_the_chunk_it_arrived_on`, `with_backend_replaces_the_backend_and_drops_its_toolset`, `a_limited_stream_is_reported_as_a_limit_after_what_was_streamed`, `a_limited_turn_halts_as_a_limit_and_a_failed_one_as_words`; update the Fable tests to expect `Limited { credits: true }`.

### P2 — CLI sessions (zone_core + chat plumbing)

Files: `zone_core/src/llm/provider/{session.rs (new), settings.rs, agent.rs, cli.rs, transcript.rs, parser/claude.rs, parser/codex.rs}`, `zone_core/src/llm/client.rs` (session chunk), `zone_server/src/services/agent/sessions.rs` (new, + `sessions/carried.rs`), `zone_server/src/services/chat/session.rs`, `zone_server/src/ws/chat.rs` (session plumbing only), `zone_server/src/db/chats.rs` (session columns; the DDL lives in P3's migration, so P2 lands after P3 or ships the two columns' queries behind P3's migration), `zone_server/tests/chat_sessions_tests.rs` (new), `zone_server/tests/agent_session_resume_tests.rs` (new, `#[ignore]`, real CLIs).

- **Spike first** (day one, before the rest of P2): two `#[ignore]` tests that need the host `claude`/`codex`: start a session under config dir A in a temp work dir, copy the session file into config dir B, resume under B, assert the answer references the first turn. If a CLI refuses cross-home resume, P2 keeps per-chat sessions but the handover uses the replay path for that agent; record the result in `docs/CONFIGURATION.md`.
- `agent.rs::arguments_with(.., session)`: Claude `--session-id <id>` (new) or `--resume <id>`; Codex `exec … -` or `exec … resume <id> -`. Parsers emit `AgentEvent::Session(id)` from `system/init` (Claude) and `thread.started` (Codex); `client.rs` maps it to `chunk.session`.
- `transcript.rs`: `render(messages)` unchanged; `render_tail(messages, from)` renders only entries after a position (used for resumed turns). Per-turn dynamic system notes ("Web search is unavailable this turn.") are rendered as a leading `System:` block of the tail, the same format as today.
- `services/chat/session.rs`: `Preparation` gains `session: Option<ChatSession>`; for a CLI backend whose chat has a session on the chosen login's agent, the run context is trimmed to entries after `entry` and `CliSettings.session = Some(Session { id, resume: true })`; otherwise the full context and `Session { id: Uuid::new_v4(), resume: false }` for Claude, or `None` for Codex (id learned from the stream).
- `ws/chat.rs`: on `AgentEvent::Session(id)` (first Codex turn) and at `finish`, `chats::set_session` with the latest persisted `chat_entries.position`; a resume that fails before `init` with `No conversation found` (Claude) / `not found` (Codex) reruns once as a fresh session with the full replay (log at info).
- `sessions.rs` per the contract; `sanitized()` reproduces Claude's project folder name for the shared `work` path.
- Tests: `a_first_turn_pins_a_session_and_a_later_turn_resumes_it_with_only_the_new_message` (stand-in `claude` records argv and stdin: turn 1 has `--session-id` and the full transcript, turn 2 has `--resume <same id>` and only the new `User:` block), `a_codex_turn_learns_its_thread_id_from_the_stream_and_resumes_it`, `a_resume_the_cli_refuses_falls_back_to_a_full_replay_under_a_fresh_id`, `a_session_file_is_carried_between_two_login_homes_under_the_same_path` (Claude and Codex layouts, 0600), `carrying_a_session_that_does_not_exist_is_reported_not_faked`, `an_http_backend_keeps_no_session`, `a_chat_on_a_new_agent_starts_a_new_session_with_the_whole_transcript`; the two `#[ignore]` spike tests.
- Note for docs: with resume, the CLI's own compaction governs the model context; Zone's checkpoint compaction applies only to the replay path.

### P3 — schema, rows, homes, config

Files: `zone_server/migrations/049_agent_login_usage.sql` (new), `src/db/agent_logins.rs`, `src/db/chats.rs` (DDL-backed queries for `agent_login_id`, `agent_session_*`), `src/config.rs`, `src/services/login/homes.rs` (new), `src/services/login/identity.rs` (new), `src/services/login/mod.rs`.

- Migration 049: `DROP CONSTRAINT agent_logins_organization_id_agent_key`; add `account TEXT`, `windows JSONB`, `headroom DOUBLE PRECISION`, `usage_fetched_at`, `exhausted_until`, `last_used_at`; index `(organization_id, agent)`; partial unique `(organization_id, agent, account) WHERE account IS NOT NULL`; the four `chats` columns above. Existing rows keep their ids.
- `db/agent_logins.rs`: row gains the columns; `Upsert` → `Insert { .., account }` with `ON CONFLICT (organization_id, agent, account) WHERE account IS NOT NULL DO UPDATE` (replaces credential/label/expires_at, clears `exhausted_until`); `get(id)`, `list(org)`, `list_for(org, agent)`, `lock(id)`, `renew`, `observe(id, &Snapshot)`, `exhaust(id, until)` (`GREATEST`), `touch(id, at)`, `delete(org, id) -> Option<row>`, `delete_all(org, agent)`. Keep production callers compiling with equivalent calls (P4/P5 rewrite them).
- `config.rs` per the contract; `<state>/<org>/<agent>` is the shared root holding `work/` and `logins/`.
- `homes.rs::adopt(state, &AgentLoginRow) -> Result<PathBuf>`: creates the login home; for codex, when the org has exactly one codex row and a regular file `<root>/auth.json` exists, renames it into `logins/<id>/` under the org's device lock.
- Tests: `an_organization_keeps_several_logins_per_agent`, `signing_in_the_same_account_again_replaces_its_login_and_clears_its_exhaustion`, `a_login_with_no_account_is_never_merged_with_another`, `a_snapshot_is_written_over_the_login_and_read_back`, `exhausting_a_login_never_shortens_an_earlier_exhaustion`, `deleting_one_login_leaves_its_siblings_and_returns_what_went`, `lock_holds_one_login_and_not_its_siblings`, `a_chat_remembers_its_login_and_session_until_that_login_goes`, `a_login_home_sits_under_the_shared_agent_root_beside_work`, `the_usage_ttl_defaults_to_a_minute_and_refuses_words`, `a_legacy_codex_login_is_moved_into_the_only_logins_home_once`, `a_legacy_login_is_left_alone_once_a_second_codex_login_exists`, `a_linked_auth_file_is_never_adopted`.

### P4 — usage snapshots, router, per-login credentials, backend resolution (needs P0 published)

Files: `src/services/login/usage.rs` + `usage/snapshot.rs` (new), `src/services/login/router.rs` + `router/chosen.rs` (new), `src/services/login/credential.rs`, `src/services/backend.rs` + `backend/resolved.rs` (new), `src/services/login/mod.rs`.

- `usage.rs`: `credential(login, resolved) -> aiusg::store::Credential` (Claude: the resolved token; Codex: `aiusg::provider::codex::discover_in(&home)`), `fetch(agent, &credential) -> Snapshot` via `aiusg::provider::{claude,codex}::fetch` (5 s timeout; windows → `zone_core::Window` via `From<aiusg::model::Window>`; headroom/usable_at from aiusg's `Usage`), `refresh(state, logins)` parallel + single-flight per login + stale fallback. **A 401/403 from a usage endpoint never removes a candidate**; the snapshot stays unknown.
- `router.rs::pick`: candidates = the preferred agent's logins, then every other agent's, minus `exclude`; a `sticky` login that is signed in and not exhausted is returned without ranking; otherwise resolve each in parallel (unrenewable → dropped), refresh stale snapshots, rank within each agent group by `Usability` (`Available` by headroom, then unknown; never `Exhausted`/`SignedOut`), ties by `last_used_at` then label; `touch`. `mark_limited`: `credits` → excluded for this turn only; else `exhaust(until = resets_at ?? snapshot.usable_at ?? now + 5 min)`. `observe` merges a window by name and recomputes headroom. `settle` refreshes after a turn.
- `credential.rs`: `resolve(state, &AgentLoginRow) -> Result<Login, Error>`; renewal lock keyed by login id; Codex home = `homes::adopt`; `Error::Deleted`.
- `backend.rs`: `for_settings` → `router::pick(preferred = settings.agent(), ..)`; `Error::None` → host fallback only when `ZONE_AGENT_HOST_LOGIN`, else `SignedOut`; `Error::Exhausted` → `Error::Limited`. `on_login` builds `CliSettings` (login home as `CLAUDE_CONFIG_DIR`/`CODEX_HOME`, `working_directory = work`, `CLAUDE_CODE_OAUTH_TOKEN` for Claude, `session`), returns `Resolved`.
- Tests (wiremock + `TEST_DATABASE_URL`): `a_claude_usage_fetch_reads_headroom_from_the_most_used_window`, `a_codex_usage_fetch_sends_the_account_id_and_bearer`, `a_refused_usage_token_leaves_the_snapshot_unknown_and_the_login_a_candidate`, `a_slow_usage_endpoint_keeps_the_stored_snapshot`, `concurrent_refreshes_of_one_login_fetch_once`, `the_login_with_the_most_headroom_starts_the_session`, `a_sticky_login_keeps_the_chat_while_it_has_headroom`, `a_sticky_login_that_another_chat_exhausted_is_left_at_the_next_turn`, `the_configured_agents_logins_come_before_another_agents_even_with_less_headroom`, `another_agents_login_serves_when_the_configured_agent_is_spent`, `an_exhausted_login_is_skipped_until_its_reset_and_the_reset_is_named_when_none_remains`, `a_login_tried_this_turn_is_not_picked_again`, `a_fresh_snapshot_is_not_fetched_again_and_a_stale_one_is`, `a_login_whose_token_cannot_be_renewed_is_never_picked`, `a_credits_limit_excludes_the_login_for_the_turn_but_does_not_exhaust_it`, `observing_a_window_from_the_stream_updates_the_snapshot_without_a_fetch`, `two_claude_logins_renew_at_once_without_waiting_on_each_other`, `a_codex_login_resolves_to_its_own_home_and_adopts_the_legacy_one`, `every_login_of_an_organization_shares_one_working_directory`, `an_organization_whose_logins_are_all_exhausted_is_limited_not_signed_out_and_never_falls_back_to_the_host`, `the_hosts_login_has_no_identity`; port the existing `credential.rs`/`backend.rs` tests to per-login rows.

### P5 — sign-in service, statuses, routes

Files: `src/services/login/claude/scope.rs`, `oauth.rs`, `devices.rs`, `codex.rs`, `codex/staging.rs`, `codex/account.rs` (new), `status.rs` + `status/login.rs` (new), `src/routes/agents.rs`, `src/routes/mod.rs`, `tests/fixtures/agents.json`, `tests/agent_login_tests.rs`.

- `scope.rs`: `INFERENCE = ["user:inference", "user:profile"]`.
- `oauth.rs::record`: after sealing, `aiusg::provider::claude::profile` (5 s; `None` on error) → `insert(Insert { label: email or plan, account: email, .. })`.
- Codex: staging under `<root>/.login/<attempt>`, `promote(into: &login_home)`; `codex/account.rs::account(auth) -> Option<Account { id, email }>`; `devices::complete` inserts the row, creates its home, promotes, `describe()` with that home; `sign_out_login` (`codex logout` in that home, `remove_dir_all`, `delete`, audit); device cancel via `DELETE …/login/attempt`; `forget` logs out every login home.
- `status.rs`: `AgentStatus.logins: Vec<LoginStatus>`; `LoginStatus { id, label, plan, state, expires_at, usage: Option<UsageStatus { windows, headroom, fetched_at, exhausted_until }>, last_used_at }`; `read()` refreshes stale snapshots through the router's TTL-guarded path and summarises the best login into the agent-level fields.
- Routes per the contract; regenerate the shared fixture.
- Tests: `an_inference_sign_in_asks_for_the_profile_too`, `a_sign_in_is_labelled_with_its_email_when_the_profile_answers`, `a_sign_in_without_the_profile_scope_is_labelled_with_its_plan`, `signing_in_the_same_account_again_replaces_its_login`, `signing_in_another_account_adds_a_second_login`, `a_second_codex_sign_in_gets_its_own_home_beside_the_first`, `a_codex_re_sign_in_of_the_same_account_replaces_its_login_and_home`, `signing_out_one_codex_login_logs_out_that_home_alone`, `forgetting_an_organization_logs_out_every_login_home`, `an_account_is_read_from_auth_json_and_its_id_token`, `a_status_lists_every_login_with_its_snapshot_and_summarises_the_best`, `a_status_serialises_to_exactly_the_shared_fixture`, `an_admin_signs_out_one_login_and_the_other_stays`, `a_member_reads_every_login_and_never_a_token`, `a_second_codex_sign_in_lands_in_its_own_home`, `cancelling_a_codex_device_sign_in_keeps_the_logins_already_held`; update `the_authorize_url_carries_the_clis_parameters_in_order_for_each_scope`.

### P6 — chat handover

Files: `src/ws/chat.rs`, `src/services/chat/session.rs`, `src/services/chat/handover.rs` + `handover/switch.rs` (new), `src/services/chat/mod.rs`, `tests/chat_handover_tests.rs` (new).

- `prepare_chat`: `for_settings(.., sticky = chat.session.login)`; a sticky login that is exhausted before the turn starts is a handover at turn start (notice, no wasted spawn). `Preparation` gains `handover: Option<Handover>` (None for HTTP/instance/host).
- `handover.rs`: `Handover { organization, preferred: AgentKind, current: Option<LoginIdentity>, session: Option<ChatSession>, tried: Vec<Uuid>, timeout }`; `next(&mut self, state, reason, limit) -> Result<Switch, String>` = `mark_limited` → `router::pick(preferred = current.agent, exclude = tried)` → same agent and a session: `sessions::carry(from_home, to_home)` → `on_login(.., Session { resume: true })`; otherwise `on_login(.., fresh)` with `Switch.replay = true` → `bounded(remaining)`. `resume(replay, partial)`: replay path only — the partial text becomes a persisted assistant entry plus the never-persisted trailing system entry `CONTINUE = "Continue the answer from where it stopped, without repeating what was already written."`; on the carried path the continue instruction is the resumed turn's only prompt. `Switch { backend, from, to, agent, reason, resets_at, carried, replay, model: Option<String> }` (model re-picked via `stages::chat_model` with `Catalog::for_backend` when the agent changed; a name the new agent doesn't know → auto).
- `handle_chat_generation`: `llm_client` mutable; arms `Window` → `router::observe` (spawned), `Usage` summed, `Limited` → pending switch, `Failed` with `remedied(..).signed_out` → switch with reason `signed_out`. On a switch: `Status { "Switching to {to}…" }`, `Handover {..}`, push onto `handovers`, `chats::set_session` (new login; same id when carried, cleared when replayed), context = tail (carried) or `resume(..)` (replayed), `llm_client = llm_client.with_backend(switch.backend)` + re-attach `served.lease.toolset()` (same MCP lease/token/cwd), `tools = mcp::offered(..)` rebuilt as the wait path does, model swapped if changed, `continue`. `Err(message)` → today's failure path. `finish` merges `handovers` and summed `usage`; `MessageEnd.content` is the whole answer under one id. `router::settle` for every login used.
- Bounds: one try per login per turn; host login → no handover; a login removed mid-turn → skipped; `credits` hands over per remaining login then ends with `UNFUNDED`; nothing streamed → rerun as is; deadline and cancel unchanged.
- Tests: `services/chat/handover.rs`: `a_replayed_resume_carries_the_partial_answer_persisted_and_the_instruction_unpersisted`, `a_carried_session_sends_only_the_instruction`, `a_switch_to_another_agent_replays_and_repicks_the_model`. `tests/chat_handover_tests.rs` (stand-in `claude` branches on `$CLAUDE_CODE_OAUTH_TOKEN` and records argv/stdin/`CLAUDE_CONFIG_DIR`; token A prints one chunk then replays `fable-limit-reached.jsonl`; token B answers; two rows via `insert`; usage endpoints wiremocked): `a_turn_that_hits_its_limit_continues_on_another_login_in_the_same_message` (one `message_start`, one `handover` frame with `at` = A's text length and `carried: true`, the session file present in B's home, B invoked with `--resume <id>` and stdin == `CONTINUE`, `message_end.content == partial + answer`, `metadata.handovers` persisted, `chats.agent_login_id == B`, same `ZONE_MCP_TOKEN` and cwd), `a_handover_without_a_session_file_replays_the_transcript` (B's stdin holds the transcript, the partial under `Assistant:` and `CONTINUE`; `carried: false`), `a_chat_whose_agent_is_spent_continues_on_the_other_agent` (Codex stand-in answers; `handover.agent == codex`; model re-picked), `a_turn_with_no_login_left_fails_naming_the_earliest_reset`, `the_next_turn_stays_on_the_login_the_last_one_ended_on`, `a_sticky_login_exhausted_elsewhere_is_left_before_the_turn_spawns`, `a_handover_never_runs_the_same_login_twice_in_one_turn`, `a_credits_limit_hands_over_and_ends_with_the_credits_wording_when_none_remains`, `a_signed_out_login_hands_over_and_says_so`.

### P7 — task handover

Files: `src/workers/task.rs`, `src/workers/task/halt.rs`.

- `Fault` gains `limit: Option<Limit>`, `login: Option<LoginIdentity>`; `Fault::limited(backend, login, limit)`. `Failure::Rerouted` / `Decision::Reroute`: no sleep, no attempt increment; run log "Attempt {n} handed over from {from} to {to} after a usage limit: {message}" with metadata `{outcome: "reroute", from, to, agent, resets_at}`.
- `execute_owned_task_run` keeps `Resolved`; `refreshed()` → `routed(state, workspace, prepared, tried)` = `for_settings(.., exclude = tried, sticky = the run's login)`; `backend::Error::Limited` → `RateLimited { retry_after: resets_at - now }`. A limited attempt: `mark_limited`, push to `tried`, re-label `Rerouted` when another candidate exists (any agent; the attempt's model is re-picked for a new agent). `SUBSCRIPTION_LIMIT_MARKERS` → `LIMIT_WORDINGS`. `settle` after each attempt.
- Tests: `a_reroute_costs_no_attempt_and_no_backoff`, `a_limit_without_another_login_backs_off_by_its_reset`, cli_tests with `Agent::limited()`: `a_run_that_hits_its_limit_reroutes_to_another_login_without_backing_off`, `a_run_stays_on_its_login_across_attempts_while_it_has_headroom`, `a_run_backs_off_only_once_every_login_is_exhausted`, `a_run_with_one_login_keeps_todays_backoff`, `a_credits_limit_ends_the_run_when_no_login_remains`.

### P8 — console (`manager/frontend`)

Files: `src/features/settings/ai/{schemas.ts, AgentSignIn.tsx, AgentSignIn.css, SignOutDialog.tsx, AccountList.tsx (new), UsageBar.tsx (new), usage.ts (new)}`, `src/api/agents.ts`, `src/features/chats/{schemas.ts, utils/handover.ts (new), components/HandoverNotice.tsx (new), components/index.ts, hooks/useChat.ts, pages/ChatsPage.tsx, pages/ChatsPage.css}`, tests beside each, `e2e/agent-sign-in.e2e.ts`, `e2e/chat-regressions.e2e.ts`, screenshots.

- Schemas: `LoginStateSchema`, `UsageWindowSchema`, `UsageSchema`, `AgentAccountSchema`, `AgentStatusSchema.logins: z.array(AgentAccountSchema).default([])`; chat `HandoverSchema { kind: 'handover', from?, to, agent, reason (catch 'limit'), resets_at?, carried, at }`.
- `agentsApi.signOut(org, agent, loginId)` → `DELETE …/logins/{id}`; Codex device cancel uses `cancel`.
- `usage.ts` mirrors aiusg's `render` (`severityOf`, `percentOf`, `countsOf`, `resetsIn`, `accountLabel`); `UsageBar` renders `role="meter"` rows in aiusg's table order (`5h ████████░░ 62% resets in 2h 10m`); tints from `packages/ui` tokens.
- `AccountList`: one row per login (label, plan, state badge, per-row Sign out with the existing dialog naming the account, usage bars, meta line: exhausted-until, expiry, last used, usage checked). `AgentSignIn`: list whenever logins exist (members read-only); explainer "New chats start on the account with the most headroom and stay on it until it runs out; the other agent's accounts take over when this one is spent."; button "Sign in with {name}" / "Add another {name} account" (same start flow, appends); agent-level Sign out removed; `USAGE_POLL_INTERVAL = 60_000` refresh while idle. Props and the region heading "{name} sign-in" stay, so the settings pages need no change.
- Chat: `useChat` handles the `handover` frame by appending to the open assistant message's `metadata.handovers`; `ChatsPage` renders the assistant content split at each `at` (code-point offsets via `Array.from`) with a `HandoverNotice` divider ("Switched to **Codex · b@example.com** — a@example.com reached its usage limit; resets in 2h 10m", `role="note"`; agent shown when it changed), live and on reload from metadata; the transient `status` text shows in `<Generation>` as today.
- Tests: update `AgentSignIn.test.tsx` (account rows, meters, per-account sign-out, device cancel calls `cancel`, member read-only, add-account appends, exhausted, usage unavailable, no limits, 60 s refresh, failed refresh keeps the reading), `api/agents.test.ts` (fixture parses with `logins`, sign-out URL, tolerant of missing `logins`), `useAgentStatuses.test.ts`, new `usage.test.ts`, `UsageBar.test.tsx`, `HandoverNotice.test.tsx` (limit/credits/signed-out/no `from`/agent change), chats `schemas.test.ts`, `ChatsPage.test.tsx` (divider inside the answer, status text, reload from metadata), `useChat.test.ts`; `OrgSettingsPage.test.tsx`/`WorkspaceSettingsPage.test.tsx` mocks gain `cancel`. Playwright: `agent-sign-in.e2e.ts` scenarios (accounts list, add another, exhausted, sign one out, member view, codex cancel) and `chat-regressions.e2e.ts` `a turn that moves to another account says so inside the reply`; regenerate and commit `screenshots/agent-sign-in-*.png` + `chats-handover.png` (Chromium).

### P9 — build, ops, docs, CodeQL

- `runner/Cargo.toml`: `reqwest = { version = "=0.13.5", .. }`; `cargo update -p reqwest --precise 0.13.5`.
- `runner/zone_server/Cargo.toml`: `aiusg = { version = "=0.5.0", default-features = false }` (zone_server, not zone_core: zone_core ships in `zone_cli`). Commit `Cargo.lock`; `cargo tree -p zone_server -i crossterm` (and webbrowser/rusqlite/keyring/clap) must not match.
- `runner/deny.toml`: nothing for sources; licenses MIT; `docs/DEPENDENCIES.md` row for aiusg (why, `default-features = false`, what stays out).
- `docker-compose.yml` manager env `ZONE_AGENT_USAGE_TTL_SECONDS=${ZONE_AGENT_USAGE_TTL_SECONDS:-60}`; `.env.example` block; `helm/zone-apps/values.yaml` comment + README rows (per-account homes, session files under each login's `projects/`/`sessions/`, network policy note that usage reads need `api.anthropic.com`/`chatgpt.com`). Dockerfile unchanged.
- `docs/CONFIGURATION.md`: state-dir layout (`logins/<id>` + shared `work`; session files live in the login home and move with a handover), `ZONE_AGENT_USAGE_TTL_SECONDS` (+ refused-at-boot list), "Signing in" (several accounts, add/sign out per account, scope `user:inference user:profile`, older sign-ins read as usage unknown until re-signed in), routes table, "How a turn runs" → "Which account runs a chat" (routing at session start, sticky, cross-agent fallback order, session resume and what a resumed turn sends, the replay fallback, handover and what the chat shows, that the CLI's own compaction governs a resumed chat), Security wording, Known gaps (undocumented usage endpoints; cross-home resume as verified by the spike). `docs/OPERATIONS.md`: archive layout and "Upgrading to migration 049". `README.md` one-liner.
- `.github/codeql/extensions/zone-models/models/barriers.yml`: `zone_server::config::agent_login_home`, `zone_server::config::AgentConfig::login_home`, and `zone_server::services::agent::sessions::locate` return-value barriers (path-injection), beside `agent_home`.
- `frontend-tests.yml` path filter: `runner/zone_server/src/services/login/status/**` if not already matched.

### Ordering

| Package | Needs | Parallel with |
|---|---|---|
| P0 aiusg | — (Jake: crates.io token) | P1, P3, P5, P8 |
| P1 core signal | — | P3 |
| P2 CLI sessions | P3 (columns), P1 (events) | P4, P5 |
| P3 schema/rows | — | P1 |
| P4 router/backend | P0 published, P1, P3 | P2, P5 |
| P5 sign-in/status/routes | P3 | P2, P4 |
| P6 chat handover | P2, P4 | P7 |
| P7 task handover | P1, P4 | P6 |
| P8 console | contract only | everything |
| P9 build/docs | P0 (Cargo line); docs any time | everything |

Merge order into the feature branch: P1 → P3 → P9 (Cargo) → P2 ‖ P4 ‖ P5 → P6 ‖ P7 → P8 → docs. The workspace must compile after each merge (P1 ships the `Fault::limited` stub; P3 keeps old callers compiling with equivalent calls).

## Execution

- Sync the branch onto `origin/main` first (`sync_with_base_branch`).
- P0 as its own PR in `~/Local/aiusg` via `shepherd`: CI green (ubuntu/macos/windows + library job) → merge → `gh release create 0.5.0` → `release.yml` green including `cargo publish`. No local shims or `[patch]`; Zone's Cargo change waits for crates.io (`/loop`).
- Run the P2 spike before fanning out, so P6's carried path is known to work (or known to fall back) before it is written.
- Run the Zone packages through the `skills:consolidation` cycle: planner → verifier → parallel architects in worktrees (one per package, ownership above) → consolidator → reviewer → verifier. Each package includes its regression tests.
- House rules: no narrating comments, single-word names where unambiguous, typed structs, constants over strings, `cargo fmt` + `clippy -D warnings` + biome before each commit, conventional commits, PR targets `main`.

## Verification

Machine notes: check `df -h` before building (the shared cargo build dir fills the disk); `CARGO_INCREMENTAL=0` for clippy/one-off runs; never `cargo clean`; if `E0514` appears check `rustc --version` (Homebrew 1.98.1 shadows rustup). Unique disposable DB container names per agent.

aiusg (`~/Local/aiusg`):
```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --all && cargo build --no-default-features && cargo clippy --no-default-features --all-targets -- -D warnings && cargo test --no-default-features && cargo build --no-default-features --features login && ! (cargo tree --no-default-features -e normal | grep -E 'crossterm|webbrowser|rusqlite|keyring|clap|rmcp|schemars') && cargo publish --dry-run
```

Zone Rust (`runner/`), with a disposable Postgres:
```bash
docker run -d --rm --name zone-test-pg-<agent> -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=zone_test -p 55432:5432 pgvector/pgvector:pg17
export TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:55432/zone_test DATABASE_URL=$TEST_DATABASE_URL
cargo fmt --all --check && CARGO_INCREMENTAL=0 cargo clippy --workspace --exclude zone_desktop --all-targets -- -D warnings
cargo nextest run -p zone_core -p zone_server
cargo nextest run -p zone_server --run-ignored ignored-only -E 'test(session_resume)'   # the spike, on a Mac signed in to claude and codex
cargo deny --locked check && cargo tree -p zone_server -i crossterm   # expect "did not match any packages"
```

Console (`manager/frontend`, never from the repo root):
```bash
bun run build:ui && bun test --isolate && bunx tsc --noEmit && bunx biome check src && bunx playwright test e2e/agent-sign-in.e2e.ts e2e/chat-regressions.e2e.ts --project=chromium
```

Image and live pass: `docker build -f manager/Dockerfile --target builder .`, then on the compose stack: migrate an existing 048 database; sign two Claude accounts in (Jake's own; confirm the grant's scope and lifetime and that `/api/oauth/usage` answers); the panel shows both with usage bars matching `aiusg` on the host; a new chat starts on the account with more headroom and its second turn is a `--resume` (server log shows the argv, and the turn's input tokens are a fraction of the first turn's); a stand-in replaying `fable-limit-reached.jsonl` for account A makes the chat show the divider and finish on B under one message with `carried: true`; sign B out and a Codex account in, exhaust Claude, and see the chat continue on Codex with the model re-picked; a task run reroutes without backoff; the pre-049 codex `auth.json` is adopted into `logins/<id>/`. Screenshot the panel and the chat handover and check them.

## Out of scope / follow-ups

- Grok agent (new `AgentKind`, binary in the image, `streaming-json` parser, sign-in, `GROK_HOME`); aiusg's `grok::{fetch, discover_in}` land now.
- Session resume for task attempts (attempts keep transcript replay).
- Falling back to the instance (self-hosted) backend when every subscription is spent.
- The credits-handling follow-up PR #98 promised is orthogonal; P1 keeps the `UNFUNDED` wording.

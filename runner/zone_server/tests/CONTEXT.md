# Conversation context tests

Run against a disposable PostgreSQL database with migrations 001 through 015 applied. `TEST_DATABASE_URL` is the existing CI setting. Optionally set `ZONE_CONTEXT_TEST_DATABASE_URL` to override it for these tests. Neither the store nor runtime acceptance fixture falls back to the application `DATABASE_URL`. Missing explicit test configuration fails clearly and never skips tests. SQLx compilation may additionally require `DATABASE_URL` pointing at the same disposable database.

```sh
TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:49755/zone_context_test \
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:49755/zone_context_test \
cargo test -p zone_server --test chat_context_tests --test chat_replay_tests --test chat_context_store_tests --test chat_lifecycle_tests --no-default-features --features zone_context/test-utils
```

The fixtures serve the real router on an ephemeral local port, authenticate through its public API, and replace inference and model metadata with deterministic Wiremock responses. They disable MCP startup and external web/image services. Tool mutations are confined to per-test temporary directories. Tests retain rows only in the disposable database; remove the database when finished.

`fixtures/context.json` is checked against Rust `ContextUsage` serialization and shared with frontend E2E tests.

## Library tests that need the same database

`cargo test -p zone_server --lib` is not all in-process. The memory tools' fixture (`agent::memory`) and `agent::tools::tests::a_run_with_an_authorized_writer_still_gets_no_memory_tool` connect to `DATABASE_URL` and expect the migrations applied, so a `--lib` run without it fails where it would otherwise pass. They are deliberately not `#[ignore]`d: CI runs nextest without `--include-ignored`, and an ignored test that proves memory stays off the task surface is a test that never runs. Point `DATABASE_URL` at the same disposable migrated database the suites above use.

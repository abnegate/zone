-- What a run's worktree may be removed against: the commit it started on,
-- and the commit the service last pushed from it. Cleanup trusts these two
-- facts, recorded by the service itself, and nothing a run's own git commands
-- can write, since every ref in a shared clone is theirs to move. One row per
-- run, made when the worktree is, so a run without a repository has none.
SET LOCAL lock_timeout = '5s';
CREATE TABLE IF NOT EXISTS task_run_checkouts (
    run_id UUID PRIMARY KEY REFERENCES task_runs(id) ON DELETE CASCADE,
    head TEXT NOT NULL,
    published TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

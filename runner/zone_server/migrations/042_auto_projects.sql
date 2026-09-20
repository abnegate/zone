-- A project may run itself: every agentic task it holds is executed, reviewed
-- and merged without anyone in the loop. The switch, the actor it runs as, the
-- brief the interview produced and the driver's lease live on the project; a
-- chat records what it is for and which project it belongs to; a run records
-- the model it used and whether anyone was watching; each task's place in the
-- review-and-merge pipeline and the reviews its pull request received get
-- tables of their own. Everything here is metadata with a default -- no scan,
-- no backfill -- and every constraint is added NOT VALID and validated in 043,
-- the way 035 adds what 036 validates.
SET LOCAL lock_timeout = '5s';

ALTER TABLE projects ADD COLUMN IF NOT EXISTS auto BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS auto_actor_id UUID;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS brief JSONB;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS auto_paused_reason TEXT;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS auto_claimed_at TIMESTAMPTZ;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS auto_completed_at TIMESTAMPTZ;
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_auto_actor_id_fkey;
ALTER TABLE projects ADD CONSTRAINT projects_auto_actor_id_fkey
    FOREIGN KEY (auto_actor_id) REFERENCES users(id) ON DELETE SET NULL NOT VALID;
CREATE INDEX IF NOT EXISTS idx_projects_auto ON projects(id) WHERE auto;

ALTER TABLE chats ADD COLUMN IF NOT EXISTS purpose TEXT NOT NULL DEFAULT 'assistant';
ALTER TABLE chats ADD COLUMN IF NOT EXISTS project_id UUID;
ALTER TABLE chats DROP CONSTRAINT IF EXISTS chats_purpose_check;
ALTER TABLE chats ADD CONSTRAINT chats_purpose_check
    CHECK (purpose IN ('assistant', 'project_planner', 'project_updates')) NOT VALID;
ALTER TABLE chats DROP CONSTRAINT IF EXISTS chats_project_id_fkey;
ALTER TABLE chats ADD CONSTRAINT chats_project_id_fkey
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL NOT VALID;
CREATE INDEX IF NOT EXISTS idx_chats_project ON chats(project_id) WHERE project_id IS NOT NULL;

ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS model TEXT;
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS unattended BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS task_automation (
    task_id UUID PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT CHECK (kind IN ('scaffold', 'ci', 'tests', 'feature', 'deployment', 'docs', 'fix')),
    stage TEXT NOT NULL DEFAULT 'idle' CHECK (stage IN (
        'idle', 'running', 'no_changes', 'awaiting_checks', 'awaiting_reviews',
        'fixing', 'merging', 'post_merge', 'merged', 'paused')),
    reason TEXT,
    runs INTEGER NOT NULL DEFAULT 0,
    review_rounds INTEGER NOT NULL DEFAULT 0,
    head TEXT,
    checks TEXT,
    checks_since TIMESTAMPTZ,
    bot_trigger_head TEXT,
    merge_sha TEXT,
    auto_created BOOLEAN NOT NULL DEFAULT FALSE,
    last_run_id UUID REFERENCES task_runs(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_task_automation_project_stage ON task_automation(project_id, stage);

CREATE TABLE IF NOT EXISTS task_reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    run_id UUID REFERENCES task_runs(id) ON DELETE SET NULL,
    round INTEGER NOT NULL,
    head TEXT NOT NULL,
    reviewer_kind TEXT NOT NULL DEFAULT 'model' CHECK (reviewer_kind IN ('model', 'bot')),
    reviewer TEXT NOT NULL,
    author_model TEXT,
    same_model BOOLEAN NOT NULL DEFAULT FALSE,
    verdict TEXT NOT NULL CHECK (verdict IN ('approve', 'request_changes', 'unparseable')),
    summary TEXT NOT NULL DEFAULT '',
    findings JSONB NOT NULL DEFAULT '[]'::jsonb,
    addressed JSONB NOT NULL DEFAULT '[]'::jsonb,
    external_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (task_id, round)
);

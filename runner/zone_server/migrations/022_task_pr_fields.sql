-- Add PR-related fields to tasks table for tracking PR creation on task completion

-- Add pr_url to store the created pull request URL
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pr_url TEXT;

-- Add branch_name to track the branch created for task changes
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS branch_name TEXT;

-- Add pr_status to track PR lifecycle: null (no PR), 'pending', 'open', 'merged', 'closed'
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pr_status TEXT;

-- Add pr_created_at timestamp
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pr_created_at TIMESTAMP;

-- Index for querying tasks with PRs
CREATE INDEX IF NOT EXISTS idx_tasks_pr_status ON tasks(pr_status) WHERE pr_status IS NOT NULL;

-- Index for querying tasks with branches
CREATE INDEX IF NOT EXISTS idx_tasks_branch_name ON tasks(branch_name) WHERE branch_name IS NOT NULL;

-- Add modified_files JSON column to task_runs to track files changed during execution
ALTER TABLE task_runs ADD COLUMN IF NOT EXISTS modified_files JSONB;

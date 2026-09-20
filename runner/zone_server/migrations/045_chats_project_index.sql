-- no-transaction
-- The planner and updates chats of a project, found by project. Built
-- concurrently for the same reason as 044; one statement per file.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_chats_project ON public.chats(project_id) WHERE project_id IS NOT NULL;

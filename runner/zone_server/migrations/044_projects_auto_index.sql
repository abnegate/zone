-- no-transaction
-- The rows the auto-project driver claims: a project with `auto` on. Built
-- concurrently so a deployment never blocks writes to projects for the length
-- of the build; exactly one statement per file, as 027 explains.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_projects_auto ON public.projects(id) WHERE auto;

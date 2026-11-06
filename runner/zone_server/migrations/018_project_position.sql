SET LOCAL lock_timeout = '5s';

ALTER TABLE public.projects ADD COLUMN "position" integer DEFAULT 0 NOT NULL;

UPDATE public.projects
SET "position" = ranked."position"
FROM (
    SELECT id, (ROW_NUMBER() OVER (PARTITION BY workspace_id ORDER BY created_at DESC, id))::integer AS "position"
    FROM public.projects
) AS ranked
WHERE projects.id = ranked.id;

CREATE INDEX idx_projects_workspace_position
    ON public.projects USING btree (workspace_id, "position");

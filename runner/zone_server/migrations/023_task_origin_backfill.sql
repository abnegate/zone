SET LOCAL lock_timeout = '5s';

UPDATE public.tasks AS task
SET origin = 'automation'
WHERE EXISTS (
        SELECT 1 FROM public.task_automation AS automation
        WHERE automation.task_id = task.id AND automation.auto_created
    )
   OR EXISTS (
        SELECT 1 FROM public.synced_items AS synced
        WHERE synced.task_id = task.id
    );

UPDATE public.tasks AS task
SET origin = CASE turn.metadata ->> 'source' WHEN 'reminder' THEN 'reminder' ELSE 'automation' END
FROM (
    SELECT created.id, (
        SELECT message.metadata
        FROM public.messages AS message
        WHERE message.chat_id = created.chat_id
          AND message.role = 'user'
          AND message.created_at <= created.created_at
        ORDER BY message.created_at DESC
        LIMIT 1
    ) AS metadata
    FROM public.tasks AS created
    WHERE created.chat_id IS NOT NULL AND created.origin = 'member'
) AS turn
WHERE task.id = turn.id
  AND jsonb_typeof(turn.metadata) = 'object'
  AND turn.metadata ? 'source';

ALTER TABLE public.tasks VALIDATE CONSTRAINT tasks_origin_check;

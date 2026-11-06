-- no-transaction
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_messages_typed
    ON public.messages USING btree (chat_id, created_at DESC)
    WHERE role = 'user'
      AND NOT COALESCE(jsonb_typeof(metadata) = 'object' AND metadata ? 'source', FALSE);

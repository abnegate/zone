SET LOCAL lock_timeout = '5s';

ALTER TABLE public.audit_logs
    ADD COLUMN transaction_id bigint DEFAULT ((pg_current_xact_id())::text)::bigint NOT NULL,
    ADD COLUMN sequence bigint GENERATED ALWAYS AS IDENTITY;

CREATE INDEX audit_logs_organization_position_idx ON public.audit_logs USING btree (organization_id, transaction_id, sequence);

CREATE INDEX audit_logs_transaction_idx ON public.audit_logs USING btree (transaction_id);

CREATE INDEX audit_logs_server_position_idx ON public.audit_logs USING btree (transaction_id, sequence) WHERE organization_id IS NULL;

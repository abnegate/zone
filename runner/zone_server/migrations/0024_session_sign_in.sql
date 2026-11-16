SET LOCAL lock_timeout = '5s';

ALTER TABLE public.sessions
    ADD COLUMN sso_provider_id uuid,
    ADD COLUMN absolute_expires_at timestamp with time zone,
    ADD CONSTRAINT sessions_sso_provider_id_fkey FOREIGN KEY (sso_provider_id) REFERENCES public.sso_providers(id) ON DELETE CASCADE;

UPDATE public.sessions SET absolute_expires_at = expires_at;

ALTER TABLE public.sessions ALTER COLUMN absolute_expires_at SET NOT NULL;

CREATE INDEX sessions_sso_provider_id_idx ON public.sessions USING btree (sso_provider_id) WHERE (sso_provider_id IS NOT NULL);

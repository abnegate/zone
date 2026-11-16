SET LOCAL lock_timeout = '5s';

CREATE INDEX users_email_lower_idx ON public.users USING btree (lower(email));

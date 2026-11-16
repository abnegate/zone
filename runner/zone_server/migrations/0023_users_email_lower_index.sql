SET LOCAL lock_timeout = '5s';

DO $$
DECLARE
    shared text;
BEGIN
    SELECT string_agg(email, ', ' ORDER BY email)
    INTO shared
    FROM (
        SELECT lower(email) AS email
        FROM public.users
        GROUP BY lower(email)
        HAVING count(*) > 1
    ) AS duplicates;
    IF shared IS NOT NULL THEN
        RAISE EXCEPTION 'More than one account uses each of these email addresses, differing only in letter case: %', shared
            USING HINT = 'Change the email of all but one account for each address, or delete the extra accounts, then start Zone again. Email addresses are now unique ignoring case.';
    END IF;
END
$$;

CREATE UNIQUE INDEX users_email_lower_key ON public.users USING btree (lower(email));

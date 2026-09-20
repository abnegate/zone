SET LOCAL lock_timeout = '5s';
ALTER TABLE invitations DROP CONSTRAINT IF EXISTS invitations_email_organization_id_key;

CREATE UNIQUE INDEX IF NOT EXISTS invitations_pending_unique
  ON invitations(email, organization_id)
  WHERE accepted_at IS NULL;

ALTER TABLE email_verification_tokens ADD COLUMN IF NOT EXISTS used_at TIMESTAMPTZ;

-- Configured audio-generation checkpoint, distinct from the image checkpoint.
ALTER TABLE organization_ai_settings
    ADD COLUMN IF NOT EXISTS model_audio TEXT;

ALTER TABLE workspace_ai_settings
    ADD COLUMN IF NOT EXISTS model_audio TEXT;

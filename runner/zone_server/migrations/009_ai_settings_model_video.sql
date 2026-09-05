-- Configured video-generation UNET, distinct from the image checkpoint.
ALTER TABLE organization_ai_settings
    ADD COLUMN IF NOT EXISTS model_video TEXT;

ALTER TABLE workspace_ai_settings
    ADD COLUMN IF NOT EXISTS model_video TEXT;

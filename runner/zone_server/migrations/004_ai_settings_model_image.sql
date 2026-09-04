-- Configured image-generation model, distinct from chat/embedding models.
ALTER TABLE organization_ai_settings
    ADD COLUMN IF NOT EXISTS model_image TEXT;

ALTER TABLE workspace_ai_settings
    ADD COLUMN IF NOT EXISTS model_image TEXT;

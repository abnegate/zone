-- Delete theme for a workspace (reset to defaults)
DELETE FROM workspace_themes
WHERE workspace_id = $1

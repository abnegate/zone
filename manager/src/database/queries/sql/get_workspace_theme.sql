-- Get theme for a workspace
SELECT id, workspace_id, primary_color_light, secondary_color_light,
       primary_color_dark, secondary_color_dark, font_family, font_size_base,
       border_radius, created_at, updated_at
FROM workspace_themes
WHERE workspace_id = $1

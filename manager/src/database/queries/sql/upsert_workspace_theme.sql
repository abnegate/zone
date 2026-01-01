-- Upsert theme for a workspace (create or update)
INSERT INTO workspace_themes (
  workspace_id, primary_color_light, secondary_color_light,
  primary_color_dark, secondary_color_dark, font_family, font_size_base,
  border_radius, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
ON CONFLICT (workspace_id) DO UPDATE SET
  primary_color_light = EXCLUDED.primary_color_light,
  secondary_color_light = EXCLUDED.secondary_color_light,
  primary_color_dark = EXCLUDED.primary_color_dark,
  secondary_color_dark = EXCLUDED.secondary_color_dark,
  font_family = EXCLUDED.font_family,
  font_size_base = EXCLUDED.font_size_base,
  border_radius = EXCLUDED.border_radius,
  updated_at = EXCLUDED.updated_at
RETURNING id, workspace_id, primary_color_light, secondary_color_light,
          primary_color_dark, secondary_color_dark, font_family, font_size_base,
          border_radius, created_at::timestamp, updated_at::timestamp
SET LOCAL lock_timeout = '5s';

ALTER TABLE workspace_themes
  ALTER COLUMN primary_color_light SET DEFAULT '#0011d9',
  ALTER COLUMN secondary_color_light SET DEFAULT '#ecf9ff',
  ALTER COLUMN primary_color_dark SET DEFAULT '#00f3ff',
  ALTER COLUMN secondary_color_dark SET DEFAULT '#ecf9ff',
  ALTER COLUMN font_family SET DEFAULT 'nunito',
  ALTER COLUMN border_radius SET DEFAULT 'large';

UPDATE workspace_themes
SET
  primary_color_light = '#0011d9',
  secondary_color_light = '#ecf9ff',
  primary_color_dark = '#00f3ff',
  secondary_color_dark = '#ecf9ff',
  font_family = 'nunito',
  border_radius = 'large'
WHERE lower(coalesce(primary_color_light, '')) = '#3b82f6'
  AND lower(coalesce(secondary_color_light, '')) = '#6366f1'
  AND lower(coalesce(primary_color_dark, '')) = '#3b82f6'
  AND lower(coalesce(secondary_color_dark, '')) = '#6366f1'
  AND font_family IS NOT DISTINCT FROM 'system'
  AND border_radius IS NOT DISTINCT FROM 'medium';

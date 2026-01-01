import birl
import database/connection.{type Connection, query_error_to_string}
import gleam/dynamic/decode
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import models/workspace_theme.{
  type BorderRadius, type FontFamily, type UpdateWorkspaceThemeRequest,
  type WorkspaceTheme, RadiusMedium, System, WorkspaceTheme,
}
import pog

// =============================================================================
// Workspace Theme Queries
// =============================================================================

/// Get theme for a workspace (returns None if no custom theme exists)
pub fn get_theme(
  db: Connection,
  workspace_id: String,
) -> Result(Option(WorkspaceTheme), String) {
  let sql =
    "SELECT id, workspace_id, primary_color_light, secondary_color_light,
            primary_color_dark, secondary_color_dark, font_family, font_size_base,
            border_radius, created_at, updated_at
     FROM workspace_themes WHERE workspace_id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(workspace_id))
  |> pog.returning(theme_row_decoder())
  |> pog.execute(db)
  |> result.map(fn(returned) { list.first(returned.rows) |> option.from_result })
  |> result.map_error(query_error_to_string)
}

/// Upsert theme for a workspace (create or update)
pub fn upsert_theme(
  db: Connection,
  workspace_id: String,
  req: UpdateWorkspaceThemeRequest,
) -> Result(WorkspaceTheme, String) {
  let now = birl.to_iso8601(birl.now())

  // Get existing theme or use defaults
  case get_theme(db, workspace_id) {
    Ok(maybe_existing) -> {
      let existing = case maybe_existing {
        Some(t) -> t
        None -> workspace_theme.default_theme(workspace_id)
      }

      // Merge request with existing/defaults
      let primary_color_light =
        option.unwrap(req.primary_color_light, existing.primary_color_light)
      let secondary_color_light =
        option.unwrap(req.secondary_color_light, existing.secondary_color_light)
      let primary_color_dark =
        option.unwrap(req.primary_color_dark, existing.primary_color_dark)
      let secondary_color_dark =
        option.unwrap(req.secondary_color_dark, existing.secondary_color_dark)
      let font_family = option.unwrap(req.font_family, existing.font_family)
      let font_size_base =
        option.unwrap(req.font_size_base, existing.font_size_base)
      let border_radius =
        option.unwrap(req.border_radius, existing.border_radius)

      let sql =
        "INSERT INTO workspace_themes (
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
                   border_radius, created_at, updated_at"

      pog.query(sql)
      |> pog.parameter(pog.text(workspace_id))
      |> pog.parameter(pog.text(primary_color_light))
      |> pog.parameter(pog.text(secondary_color_light))
      |> pog.parameter(pog.text(primary_color_dark))
      |> pog.parameter(pog.text(secondary_color_dark))
      |> pog.parameter(
        pog.text(workspace_theme.font_family_to_string(font_family)),
      )
      |> pog.parameter(pog.text(font_size_base))
      |> pog.parameter(
        pog.text(workspace_theme.border_radius_to_string(border_radius)),
      )
      |> pog.parameter(pog.text(now))
      |> pog.parameter(pog.text(now))
      |> pog.returning(theme_row_decoder())
      |> pog.execute(db)
      |> result.map(fn(returned) {
        case list.first(returned.rows) {
          Ok(theme) -> theme
          Error(_) -> panic as "Upsert should return a row"
        }
      })
      |> result.map_error(query_error_to_string)
    }
    Error(err) -> Error(err)
  }
}

/// Delete theme for a workspace (reset to defaults)
pub fn delete_theme(
  db: Connection,
  workspace_id: String,
) -> Result(Bool, String) {
  let sql = "DELETE FROM workspace_themes WHERE workspace_id = $1"

  pog.query(sql)
  |> pog.parameter(pog.text(workspace_id))
  |> pog.execute(db)
  |> result.map(fn(returned) { returned.count > 0 })
  |> result.map_error(query_error_to_string)
}

// =============================================================================
// Row Decoders
// =============================================================================

fn theme_row_decoder() -> decode.Decoder(WorkspaceTheme) {
  use id <- decode.field(0, decode.string)
  use workspace_id <- decode.field(1, decode.string)
  use primary_color_light <- decode.field(2, decode.string)
  use secondary_color_light <- decode.field(3, decode.string)
  use primary_color_dark <- decode.field(4, decode.string)
  use secondary_color_dark <- decode.field(5, decode.string)
  use font_family_str <- decode.field(6, decode.string)
  use font_size_base <- decode.field(7, decode.string)
  use border_radius_str <- decode.field(8, decode.string)
  use created_at <- decode.field(9, decode.string)
  use updated_at <- decode.field(10, decode.string)

  let font_family: FontFamily = case
    workspace_theme.font_family_from_string(font_family_str)
  {
    Ok(f) -> f
    Error(_) -> System
  }

  let border_radius: BorderRadius = case
    workspace_theme.border_radius_from_string(border_radius_str)
  {
    Ok(r) -> r
    Error(_) -> RadiusMedium
  }

  decode.success(WorkspaceTheme(
    id: id,
    workspace_id: workspace_id,
    primary_color_light: primary_color_light,
    secondary_color_light: secondary_color_light,
    primary_color_dark: primary_color_dark,
    secondary_color_dark: secondary_color_dark,
    font_family: font_family,
    font_size_base: font_size_base,
    border_radius: border_radius,
    created_at: created_at,
    updated_at: updated_at,
  ))
}

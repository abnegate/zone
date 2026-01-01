import birl
import database/connection.{type Connection, query_error_to_string}
import database/queries/sql
import gleam/list
import gleam/option.{type Option, None, Some}
import gleam/result
import gleam/time/timestamp
import models/workspace_theme.{
  type BorderRadius, type FontFamily, type UpdateWorkspaceThemeRequest,
  type WorkspaceTheme, RadiusMedium, System, WorkspaceTheme,
}
import youid/uuid

// =============================================================================
// Workspace Theme Queries (using Squirrel-generated SQL)
// =============================================================================

/// Get theme for a workspace (returns None if no custom theme exists)
pub fn get_theme(
  db: Connection,
  workspace_id: String,
) -> Result(Option(WorkspaceTheme), String) {
  case uuid.from_string(workspace_id) {
    Ok(uuid_id) ->
      sql.get_workspace_theme(db, uuid_id)
      |> result.map(fn(returned) {
        list.first(returned.rows)
        |> result.map(get_theme_row_to_theme)
        |> option.from_result
      })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Upsert theme for a workspace (create or update)
pub fn upsert_theme(
  db: Connection,
  workspace_id: String,
  req: UpdateWorkspaceThemeRequest,
) -> Result(WorkspaceTheme, String) {
  case uuid.from_string(workspace_id) {
    Ok(uuid_id) -> {
      // Get existing theme or use defaults
      case get_theme(db, workspace_id) {
        Ok(maybe_existing) -> {
          let existing = case maybe_existing {
            Some(t) -> t
            None -> workspace_theme.default_theme(workspace_id)
          }

          let now = timestamp.system_time()

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

          sql.upsert_workspace_theme(
            db,
            uuid_id,
            primary_color_light,
            secondary_color_light,
            primary_color_dark,
            secondary_color_dark,
            workspace_theme.font_family_to_string(font_family),
            font_size_base,
            workspace_theme.border_radius_to_string(border_radius),
            now,
            now,
          )
          |> result.map(fn(returned) {
            case list.first(returned.rows) {
              Ok(row) -> upsert_theme_row_to_theme(row)
              Error(_) -> panic as "Upsert should return a row"
            }
          })
          |> result.map_error(query_error_to_string)
        }
        Error(err) -> Error(err)
      }
    }
    Error(_) -> Error("Invalid UUID format")
  }
}

/// Delete theme for a workspace (reset to defaults)
pub fn delete_theme(
  db: Connection,
  workspace_id: String,
) -> Result(Bool, String) {
  case uuid.from_string(workspace_id) {
    Ok(uuid_id) ->
      sql.delete_workspace_theme(db, uuid_id)
      |> result.map(fn(returned) { returned.count > 0 })
      |> result.map_error(query_error_to_string)
    Error(_) -> Error("Invalid UUID format")
  }
}

// =============================================================================
// Row Mapping Helpers
// =============================================================================

fn timestamp_to_string(ts: Option(timestamp.Timestamp)) -> String {
  case ts {
    Some(t) -> {
      let #(seconds, _nanoseconds) = timestamp.to_unix_seconds_and_nanoseconds(t)
      birl.from_unix(seconds) |> birl.to_iso8601
    }
    None -> ""
  }
}

fn font_family_from_string(s: String) -> FontFamily {
  case workspace_theme.font_family_from_string(s) {
    Ok(f) -> f
    Error(_) -> System
  }
}

fn border_radius_from_string(s: String) -> BorderRadius {
  case workspace_theme.border_radius_from_string(s) {
    Ok(r) -> r
    Error(_) -> RadiusMedium
  }
}

fn get_theme_row_to_theme(row: sql.GetWorkspaceThemeRow) -> WorkspaceTheme {
  WorkspaceTheme(
    id: uuid.to_string(row.id),
    workspace_id: uuid.to_string(row.workspace_id),
    primary_color_light: row.primary_color_light,
    secondary_color_light: row.secondary_color_light,
    primary_color_dark: row.primary_color_dark,
    secondary_color_dark: row.secondary_color_dark,
    font_family: font_family_from_string(row.font_family),
    font_size_base: row.font_size_base,
    border_radius: border_radius_from_string(row.border_radius),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

fn upsert_theme_row_to_theme(row: sql.UpsertWorkspaceThemeRow) -> WorkspaceTheme {
  WorkspaceTheme(
    id: uuid.to_string(row.id),
    workspace_id: uuid.to_string(row.workspace_id),
    primary_color_light: row.primary_color_light,
    secondary_color_light: row.secondary_color_light,
    primary_color_dark: row.primary_color_dark,
    secondary_color_dark: row.secondary_color_dark,
    font_family: font_family_from_string(row.font_family),
    font_size_base: row.font_size_base,
    border_radius: border_radius_from_string(row.border_radius),
    created_at: timestamp_to_string(row.created_at),
    updated_at: timestamp_to_string(row.updated_at),
  )
}

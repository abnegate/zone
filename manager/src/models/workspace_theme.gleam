import gleam/dynamic/decode
import gleam/json
import gleam/option.{type Option, None, Some}

/// Font family options (preset list)
pub type FontFamily {
  System
  Inter
  Roboto
  OpenSans
  Lato
  Nunito
}

/// Border radius options
pub type BorderRadius {
  RadiusNone
  RadiusSmall
  RadiusMedium
  RadiusLarge
}

/// Workspace theme entity
pub type WorkspaceTheme {
  WorkspaceTheme(
    id: String,
    workspace_id: String,
    primary_color_light: String,
    secondary_color_light: String,
    primary_color_dark: String,
    secondary_color_dark: String,
    font_family: FontFamily,
    font_size_base: String,
    border_radius: BorderRadius,
    created_at: String,
    updated_at: String,
  )
}

/// Request to update workspace theme (all fields optional, uses upsert)
pub type UpdateWorkspaceThemeRequest {
  UpdateWorkspaceThemeRequest(
    primary_color_light: Option(String),
    secondary_color_light: Option(String),
    primary_color_dark: Option(String),
    secondary_color_dark: Option(String),
    font_family: Option(FontFamily),
    font_size_base: Option(String),
    border_radius: Option(BorderRadius),
  )
}

/// Default theme values
pub fn default_theme(workspace_id: String) -> WorkspaceTheme {
  WorkspaceTheme(
    id: "",
    workspace_id: workspace_id,
    primary_color_light: "#3b82f6",
    secondary_color_light: "#6366f1",
    primary_color_dark: "#3b82f6",
    secondary_color_dark: "#6366f1",
    font_family: System,
    font_size_base: "16px",
    border_radius: RadiusMedium,
    created_at: "",
    updated_at: "",
  )
}

/// Convert FontFamily to database string
pub fn font_family_to_string(font: FontFamily) -> String {
  case font {
    System -> "system"
    Inter -> "inter"
    Roboto -> "roboto"
    OpenSans -> "open-sans"
    Lato -> "lato"
    Nunito -> "nunito"
  }
}

/// Parse FontFamily from database string
pub fn font_family_from_string(s: String) -> Result(FontFamily, Nil) {
  case s {
    "system" -> Ok(System)
    "inter" -> Ok(Inter)
    "roboto" -> Ok(Roboto)
    "open-sans" -> Ok(OpenSans)
    "lato" -> Ok(Lato)
    "nunito" -> Ok(Nunito)
    _ -> Error(Nil)
  }
}

/// Convert BorderRadius to database string
pub fn border_radius_to_string(radius: BorderRadius) -> String {
  case radius {
    RadiusNone -> "none"
    RadiusSmall -> "small"
    RadiusMedium -> "medium"
    RadiusLarge -> "large"
  }
}

/// Parse BorderRadius from database string
pub fn border_radius_from_string(s: String) -> Result(BorderRadius, Nil) {
  case s {
    "none" -> Ok(RadiusNone)
    "small" -> Ok(RadiusSmall)
    "medium" -> Ok(RadiusMedium)
    "large" -> Ok(RadiusLarge)
    _ -> Error(Nil)
  }
}

/// Decode UpdateWorkspaceThemeRequest from JSON
pub fn decode_update_request(
  data: String,
) -> Result(UpdateWorkspaceThemeRequest, json.DecodeError) {
  let decoder = {
    use primary_color_light <- decode.optional_field(
      "primary_color_light",
      None,
      decode.optional(decode.string),
    )
    use secondary_color_light <- decode.optional_field(
      "secondary_color_light",
      None,
      decode.optional(decode.string),
    )
    use primary_color_dark <- decode.optional_field(
      "primary_color_dark",
      None,
      decode.optional(decode.string),
    )
    use secondary_color_dark <- decode.optional_field(
      "secondary_color_dark",
      None,
      decode.optional(decode.string),
    )
    use font_family_str <- decode.optional_field(
      "font_family",
      None,
      decode.optional(decode.string),
    )
    use font_size_base <- decode.optional_field(
      "font_size_base",
      None,
      decode.optional(decode.string),
    )
    use border_radius_str <- decode.optional_field(
      "border_radius",
      None,
      decode.optional(decode.string),
    )

    let font_family = case font_family_str {
      Some(s) ->
        case font_family_from_string(s) {
          Ok(f) -> Some(f)
          Error(_) -> None
        }
      None -> None
    }

    let border_radius = case border_radius_str {
      Some(s) ->
        case border_radius_from_string(s) {
          Ok(r) -> Some(r)
          Error(_) -> None
        }
      None -> None
    }

    decode.success(UpdateWorkspaceThemeRequest(
      primary_color_light: primary_color_light,
      secondary_color_light: secondary_color_light,
      primary_color_dark: primary_color_dark,
      secondary_color_dark: secondary_color_dark,
      font_family: font_family,
      font_size_base: font_size_base,
      border_radius: border_radius,
    ))
  }

  json.parse(data, decoder)
}

/// Convert WorkspaceTheme to JSON
pub fn to_json(theme: WorkspaceTheme) -> json.Json {
  json.object([
    #("id", json.string(theme.id)),
    #("workspace_id", json.string(theme.workspace_id)),
    #("primary_color_light", json.string(theme.primary_color_light)),
    #("secondary_color_light", json.string(theme.secondary_color_light)),
    #("primary_color_dark", json.string(theme.primary_color_dark)),
    #("secondary_color_dark", json.string(theme.secondary_color_dark)),
    #("font_family", json.string(font_family_to_string(theme.font_family))),
    #("font_size_base", json.string(theme.font_size_base)),
    #(
      "border_radius",
      json.string(border_radius_to_string(theme.border_radius)),
    ),
    #("created_at", json.string(theme.created_at)),
    #("updated_at", json.string(theme.updated_at)),
  ])
}

/// Decoder for WorkspaceTheme (for cache deserialization)
pub fn decoder() -> decode.Decoder(WorkspaceTheme) {
  use id <- decode.field("id", decode.string)
  use workspace_id <- decode.field("workspace_id", decode.string)
  use primary_color_light <- decode.field("primary_color_light", decode.string)
  use secondary_color_light <- decode.field(
    "secondary_color_light",
    decode.string,
  )
  use primary_color_dark <- decode.field("primary_color_dark", decode.string)
  use secondary_color_dark <- decode.field(
    "secondary_color_dark",
    decode.string,
  )
  use font_family_str <- decode.field("font_family", decode.string)
  use font_size_base <- decode.field("font_size_base", decode.string)
  use border_radius_str <- decode.field("border_radius", decode.string)
  use created_at <- decode.field("created_at", decode.string)
  use updated_at <- decode.field("updated_at", decode.string)

  let font_family = case font_family_from_string(font_family_str) {
    Ok(f) -> f
    Error(_) -> System
  }

  let border_radius = case border_radius_from_string(border_radius_str) {
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

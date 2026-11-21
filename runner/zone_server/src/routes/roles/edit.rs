use serde::Deserialize;

/// A sparse edit of a custom role. A role's scope never changes, so naming it is refused with
/// every other unknown key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "crate::db::actions::patch")]
    pub description: Option<Option<String>>,
    pub permissions: Option<Vec<String>>,
}

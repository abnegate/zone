use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::limits::Limits;
use super::role::Role;
use super::validation::{manifest_path, name, relative_path};

const KIND: &str = "kind";
const MANIFEST_PATH: &str = "manifestPath";
const NAME: &str = "name";
const PATH: &str = "path";
const ROLE: &str = "role";
const SOURCE_PATH: &str = "sourcePath";
const TERMS: &str = "terms";

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    File,
    Grep,
    Script,
    Tool,
}

/// One read-only check the model nominates for the server to consider.
///
/// A recipe never carries a command. It points at source the server can look
/// at, or at a declared script the server itself resolves and executes, so the
/// model never chooses what runs.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recipe {
    File {
        path: String,
        role: Role,
    },
    Grep {
        path: String,
        terms: Vec<String>,
    },
    Script {
        #[serde(rename = "manifestPath")]
        manifest_path: String,
        name: String,
    },
    Tool {
        name: String,
        #[serde(rename = "sourcePath")]
        source_path: String,
    },
}

impl Recipe {
    pub(super) fn parse(value: &Value, limits: &Limits) -> Option<Self> {
        let object = value.as_object()?;
        match Kind::deserialize(object.get(KIND)?).ok()? {
            Kind::File => Self::file(object, limits),
            Kind::Grep => Self::grep(object, limits),
            Kind::Script => Self::script(object, limits),
            Kind::Tool => Self::tool(object, limits),
        }
    }

    fn file(object: &Map<String, Value>, limits: &Limits) -> Option<Self> {
        if !exact(object, &[KIND, PATH, ROLE]) {
            return None;
        }
        let path = text(object, PATH)?;
        if !relative_path(path, limits.field_bytes) {
            return None;
        }
        Some(Self::File {
            path: path.to_string(),
            role: Role::deserialize(object.get(ROLE)?).ok()?,
        })
    }

    fn grep(object: &Map<String, Value>, limits: &Limits) -> Option<Self> {
        if !exact(object, &[KIND, PATH, TERMS]) {
            return None;
        }
        let path = text(object, PATH)?;
        if !relative_path(path, limits.field_bytes) {
            return None;
        }
        let rows = object.get(TERMS)?.as_array()?;
        if rows.is_empty() || rows.len() > limits.terms {
            return None;
        }
        let mut terms: Vec<String> = Vec::with_capacity(rows.len());
        for row in rows {
            let term = row.as_str()?;
            if !name(term, limits.field_bytes) || terms.iter().any(|seen| seen == term) {
                return None;
            }
            terms.push(term.to_string());
        }
        Some(Self::Grep {
            path: path.to_string(),
            terms,
        })
    }

    fn script(object: &Map<String, Value>, limits: &Limits) -> Option<Self> {
        if !exact(object, &[KIND, MANIFEST_PATH, NAME]) {
            return None;
        }
        let manifest = text(object, MANIFEST_PATH)?;
        let script = text(object, NAME)?;
        if !manifest_path(manifest, limits.field_bytes) || !name(script, limits.field_bytes) {
            return None;
        }
        Some(Self::Script {
            manifest_path: manifest.to_string(),
            name: script.to_string(),
        })
    }

    fn tool(object: &Map<String, Value>, limits: &Limits) -> Option<Self> {
        if !exact(object, &[KIND, NAME, SOURCE_PATH]) {
            return None;
        }
        let tool = text(object, NAME)?;
        let source = text(object, SOURCE_PATH)?;
        if !name(tool, limits.field_bytes) || !relative_path(source, limits.field_bytes) {
            return None;
        }
        Some(Self::Tool {
            name: tool.to_string(),
            source_path: source.to_string(),
        })
    }
}

impl<'de> Deserialize<'de> for Recipe {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::parse(&value, &Limits::DEFAULT).ok_or_else(|| {
            D::Error::custom("The verification recipe is not a valid read-only nomination.")
        })
    }
}

fn exact(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn text<'value>(object: &'value Map<String, Value>, key: &str) -> Option<&'value str> {
    object.get(key)?.as_str()
}

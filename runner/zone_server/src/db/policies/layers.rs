use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub struct Layers {
    pub organization: Option<Value>,
    pub workspace: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    #[error("The scope {scope:?} is not type:name:action[,action...].")]
    Malformed { scope: String },
}

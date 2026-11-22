use crate::db::fleet::runners::Runner;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Listed {
    #[sqlx(flatten)]
    pub runner: Runner,
    /// Task runs running or waiting on this runner.
    pub runs: i64,
}

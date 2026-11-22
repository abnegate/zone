use crate::db::fleet::runners::Runner;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Registered {
    #[sqlx(flatten)]
    pub runner: Runner,
    pub created: bool,
    /// Another process held the name within [`super::CONNECTED`]: two live servers share a name,
    /// or this one restarted quickly.
    pub contested: bool,
}

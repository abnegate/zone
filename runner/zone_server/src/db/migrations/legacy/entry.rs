/// One row of `_sqlx_migrations`.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Entry {
    pub version: i64,
    pub checksum: Vec<u8>,
    pub success: bool,
}

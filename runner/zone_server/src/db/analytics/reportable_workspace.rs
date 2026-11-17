use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ReportableWorkspace {
    pub workspace_id: Uuid,
    pub name: String,
}

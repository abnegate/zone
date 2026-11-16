use uuid::Uuid;

/// A workspace with finished runs worth reporting on.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ReportableWorkspace {
    pub workspace_id: Uuid,
    pub name: String,
}

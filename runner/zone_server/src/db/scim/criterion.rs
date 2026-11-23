use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Criterion {
    Id(Uuid),
    UserName(String),
    ExternalId(String),
    DisplayName(String),
    Member(Uuid),
}

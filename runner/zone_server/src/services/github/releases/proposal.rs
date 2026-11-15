use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Proposal {
    pub(crate) source_id: Uuid,
    pub(crate) tag: String,
    pub(crate) expected_latest_tag: Option<String>,
}

use uuid::Uuid;
use zone_enterprise::spend::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub organization: Uuid,
    pub range: Range,
    pub workspace: Option<Uuid>,
    pub user: Option<Uuid>,
}

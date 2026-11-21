use uuid::Uuid;
use zone_enterprise::spend::Range;

/// The spend a report covers: an organization's, over whole UTC days, optionally narrowed to one
/// workspace and one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filter {
    pub organization: Uuid,
    pub range: Range,
    pub workspace: Option<Uuid>,
    pub user: Option<Uuid>,
}

use uuid::Uuid;

/// The runner claiming work, the process claiming as it, and whether fleet policy applies to the
/// claim. Production code gets one only from `services::fleet::claimant`, so a claim can't skip
/// enforcement by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claimant {
    pub runner: Option<Uuid>,
    pub instance: Uuid,
    pub enforced: bool,
}

impl Claimant {
    #[cfg(any(test, feature = "testing"))]
    pub const UNENFORCED: Self = Self {
        runner: None,
        instance: Uuid::nil(),
        enforced: false,
    };
}

use uuid::Uuid;

/// The runner claiming work, and whether fleet policy applies to the claim. Production code gets
/// one only from `services::fleet::claimant`, so a claim can't skip enforcement by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claimant {
    pub runner: Option<Uuid>,
    pub enforced: bool,
}

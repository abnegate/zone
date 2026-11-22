use super::SignOut;
use crate::db::invitations::Revoked;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprovisioned {
    pub sign_out: SignOut,
    pub invitations: Vec<Revoked>,
}

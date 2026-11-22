/// How a sign-in takes over the existing account it links to by email. A pristine account is
/// rechecked under lock, because only an untouched SCIM account may link on an email the identity
/// provider does not vouch for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Verified,
    Reclaim,
    Pristine,
}

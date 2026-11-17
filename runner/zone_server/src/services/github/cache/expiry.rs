#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Expiry {
    Wait,
    Revalidate,
}

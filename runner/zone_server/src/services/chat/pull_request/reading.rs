/// Why a pull request is being read from GitHub: a listing shows its review
/// threads, while a change only needs what it starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Listing,
    Change,
}

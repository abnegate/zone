/// A slice of a listing: the rows after the first `offset`, at most `limit`
/// of them, or all of them when `limit` is absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: i64,
}

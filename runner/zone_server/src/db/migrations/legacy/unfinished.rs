use thiserror::Error;

/// A database part of the way through the migrations the v1 baseline replaced.
#[derive(Debug, Error, PartialEq, Eq)]
#[error(
    "this database stopped partway through the migrations from before the v1 baseline \
     (missing {missing:?}, failed {failed:?}, unrecognised {unexpected:?}); start a Zone build \
     from before v1 (commit 218739e7) against it once to finish them, then upgrade"
)]
pub struct Unfinished {
    pub missing: Vec<i64>,
    pub failed: Vec<i64>,
    pub unexpected: Vec<i64>,
}

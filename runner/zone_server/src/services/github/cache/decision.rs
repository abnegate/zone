use super::super::freshness::Freshness;
use super::super::snapshot::Snapshot;

pub(super) enum Decision<T> {
    Serve(Snapshot<T>, Freshness),
    Refuse(String),
    Fetch,
}

impl<T> Decision<T> {
    pub(super) fn answer(self) -> Option<Result<(Snapshot<T>, Freshness), String>> {
        match self {
            Decision::Serve(snapshot, freshness) => Some(Ok((snapshot, freshness))),
            Decision::Refuse(error) => Some(Err(error)),
            Decision::Fetch => None,
        }
    }
}

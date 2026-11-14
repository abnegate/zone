use super::super::fault::Fault;
use super::super::freshness::Freshness;
use super::super::snapshot::Snapshot;

pub(super) enum Decision<T> {
    Serve(Snapshot<T>, Freshness),
    Refuse(Fault),
    Fetch,
}

impl<T> Decision<T> {
    pub(super) fn answer(self) -> Option<Result<(Snapshot<T>, Freshness), Fault>> {
        match self {
            Decision::Serve(snapshot, freshness) => Some(Ok((snapshot, freshness))),
            Decision::Refuse(fault) => Some(Err(fault)),
            Decision::Fetch => None,
        }
    }
}

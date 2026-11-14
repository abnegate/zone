use serde::Serialize;

use super::PullRequest;
use super::Readiness;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub ready: u32,
    pub in_progress: u32,
    pub not_ready: u32,
    pub total: u32,
}

impl Counts {
    pub fn of<'a>(pull_requests: impl IntoIterator<Item = &'a PullRequest>) -> Self {
        pull_requests
            .into_iter()
            .fold(Self::default(), |mut counts, pull_request| {
                match pull_request.readiness {
                    Readiness::Ready => counts.ready += 1,
                    Readiness::InProgress => counts.in_progress += 1,
                    Readiness::NotReady => counts.not_ready += 1,
                }
                counts.total += 1;
                counts
            })
    }
}

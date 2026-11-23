use std::cmp::Reverse;

use uuid::Uuid;

use super::candidate::Candidate;

pub fn choose(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates.iter().min_by_key(|candidate| rank(candidate))
}

fn rank(candidate: &Candidate) -> (Reverse<usize>, bool, String, Option<Uuid>) {
    (
        Reverse(candidate.permissions()),
        candidate.is_custom(),
        candidate.name().to_lowercase(),
        candidate.id(),
    )
}

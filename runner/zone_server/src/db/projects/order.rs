//! How a requested order is laid over a workspace's current one.

use std::collections::HashSet;

use uuid::Uuid;

/// What a reorder did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reorder {
    Reordered,
    /// The order named a project twice, so it says two things about one place.
    Duplicated,
}

/// The workspace's projects once the ones `requested` names take, in that
/// order, the places they hold in `current`. Ids `current` lacks are skipped;
/// an id named twice makes the order unusable.
pub fn arrange(current: &[Uuid], requested: &[Uuid]) -> Option<Vec<Uuid>> {
    let mut seen = HashSet::with_capacity(requested.len());
    if !requested.iter().all(|id| seen.insert(*id)) {
        return None;
    }
    let placed: Vec<Uuid> = requested
        .iter()
        .copied()
        .filter(|id| current.contains(id))
        .collect();
    let mut next = placed.iter();
    Some(
        current
            .iter()
            .map(|id| {
                if placed.contains(id) {
                    *next.next().unwrap_or(id)
                } else {
                    *id
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(count: usize) -> Vec<Uuid> {
        (0..count).map(|_| Uuid::new_v4()).collect()
    }

    #[test]
    fn a_full_order_replaces_the_current_one() {
        let current = ids(3);
        let requested = vec![current[2], current[0], current[1]];

        assert_eq!(arrange(&current, &requested), Some(requested));
    }

    #[test]
    fn a_partial_order_moves_only_the_projects_it_names() {
        let current = ids(4);
        let requested = vec![current[3], current[1]];

        assert_eq!(
            arrange(&current, &requested),
            Some(vec![current[0], current[3], current[2], current[1]]),
            "the hidden projects at the first and third places stay put"
        );
    }

    #[test]
    fn an_empty_order_changes_nothing() {
        let current = ids(2);

        assert_eq!(arrange(&current, &[]), Some(current));
    }

    #[test]
    fn an_order_naming_a_project_twice_is_refused() {
        let current = ids(2);

        assert_eq!(arrange(&current, &[current[0], current[0]]), None);
    }

    #[test]
    fn a_project_the_workspace_no_longer_holds_is_skipped() {
        let current = ids(3);
        let gone = Uuid::new_v4();

        assert_eq!(
            arrange(&current, &[current[2], gone, current[0]]),
            Some(vec![current[2], current[1], current[0]])
        );
    }
}

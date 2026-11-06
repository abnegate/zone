//! How a requested order is laid over a workspace's current one.

use std::collections::HashSet;

use uuid::Uuid;

/// What a reorder did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reorder {
    Reordered,
    /// The order named a project twice, or one the workspace does not hold.
    Unknown,
}

/// The workspace's projects once `requested` takes the places its projects
/// hold in `current`, or none when `requested` names a project twice or one
/// `current` lacks.
pub fn arrange(current: &[Uuid], requested: &[Uuid]) -> Option<Vec<Uuid>> {
    let named: HashSet<Uuid> = requested.iter().copied().collect();
    if named.len() != requested.len() || !named.iter().all(|id| current.contains(id)) {
        return None;
    }
    let mut placed = requested.iter();
    Some(
        current
            .iter()
            .map(|id| {
                if named.contains(id) {
                    *placed.next().unwrap_or(id)
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
    fn an_order_naming_another_workspaces_project_is_refused() {
        let current = ids(2);

        assert_eq!(arrange(&current, &[current[0], Uuid::new_v4()]), None);
    }
}

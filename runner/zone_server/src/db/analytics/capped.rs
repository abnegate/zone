/// Rows read under a cap, and whether the cap cut any of them off.
#[derive(Debug, Clone, PartialEq)]
pub struct Capped<T> {
    pub rows: Vec<T>,
    pub truncated: bool,
}

impl<T> Capped<T> {
    /// How many rows to ask for under a cap of `limit`: one past it, so that
    /// reaching the cap exactly is not mistaken for passing it.
    pub fn reading(limit: i64) -> i64 {
        limit.saturating_add(1)
    }

    /// At most `limit` of `rows`, read with [`Capped::reading`].
    pub fn new(mut rows: Vec<T>, limit: i64) -> Self {
        let limit = usize::try_from(limit).unwrap_or_default();
        let truncated = rows.len() > limit;
        rows.truncate(limit);
        Self { rows, truncated }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_the_cap_is_not_truncated() {
        let capped = Capped::new(vec![1, 2, 3], 3);
        assert_eq!(capped.rows, vec![1, 2, 3]);
        assert!(!capped.truncated);
    }

    #[test]
    fn one_past_the_cap_is_truncated_to_the_cap() {
        let capped = Capped::new(vec![1, 2, 3, 4], 3);
        assert_eq!(capped.rows, vec![1, 2, 3]);
        assert!(capped.truncated);
    }

    #[test]
    fn under_the_cap_keeps_everything() {
        let capped = Capped::new(vec![1], 3);
        assert_eq!(capped.rows, vec![1]);
        assert!(!capped.truncated);
    }

    #[test]
    fn the_read_asks_for_one_past_the_cap_without_overflowing() {
        assert_eq!(Capped::<()>::reading(10_000), 10_001);
        assert_eq!(Capped::<()>::reading(i64::MAX), i64::MAX);
    }

    #[test]
    fn a_negative_cap_keeps_nothing() {
        let capped = Capped::new(vec![1], -1);
        assert!(capped.rows.is_empty());
        assert!(capped.truncated);
    }
}

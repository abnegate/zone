use chrono::{DateTime, Utc};

/// When a login can take work again, ordered from soonest to least known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Availability {
    /// No window is spent.
    Now,
    /// Every spent window has reset by this time.
    At(DateTime<Utc>),
    /// A spent window reports no reset time.
    Unknown,
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    #[test]
    fn availability_orders_now_then_by_time_then_unknown() {
        let soon = Utc::now();
        let later = soon + TimeDelta::hours(1);
        let mut order = vec![
            Availability::Unknown,
            Availability::At(later),
            Availability::Now,
            Availability::At(soon),
        ];

        order.sort();

        assert_eq!(
            order,
            [
                Availability::Now,
                Availability::At(soon),
                Availability::At(later),
                Availability::Unknown,
            ]
        );
    }
}

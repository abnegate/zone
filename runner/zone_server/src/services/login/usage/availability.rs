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

impl From<aiusg::model::Availability> for Availability {
    fn from(availability: aiusg::model::Availability) -> Self {
        match availability {
            aiusg::model::Availability::Now => Self::Now,
            aiusg::model::Availability::At(at) => Self::At(at),
            aiusg::model::Availability::Unknown => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    #[test]
    fn aiusgs_availability_keeps_its_meaning() {
        let at = Utc::now();

        assert_eq!(
            Availability::from(aiusg::model::Availability::Now),
            Availability::Now
        );
        assert_eq!(
            Availability::from(aiusg::model::Availability::At(at)),
            Availability::At(at)
        );
        assert_eq!(
            Availability::from(aiusg::model::Availability::Unknown),
            Availability::Unknown
        );
    }

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

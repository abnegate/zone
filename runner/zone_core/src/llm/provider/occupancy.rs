//! How full a coding agent's context was when its turn ended.

/// The context window of the model an agent ran a turn on, and how many tokens
/// of it the turn's last request filled, both as the agent itself reported
/// them rather than as zone would estimate them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occupancy {
    pub window: u64,
    pub used: Option<u64>,
}

impl Occupancy {
    /// `window` and `used` as an agent reported them, when it named a window.
    pub fn reported(window: Option<u64>, used: Option<u64>) -> Option<Self> {
        let window = window.filter(|window| *window > 0)?;
        Some(Self {
            window,
            used: used.filter(|used| *used > 0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_needed_and_zero_counts_as_unreported() {
        assert_eq!(Occupancy::reported(None, Some(10)), None);
        assert_eq!(Occupancy::reported(Some(0), Some(10)), None);
        assert_eq!(
            Occupancy::reported(Some(200_000), Some(0)),
            Some(Occupancy {
                window: 200_000,
                used: None
            })
        );
        assert_eq!(
            Occupancy::reported(Some(200_000), Some(18_400)),
            Some(Occupancy {
                window: 200_000,
                used: Some(18_400)
            })
        );
    }
}

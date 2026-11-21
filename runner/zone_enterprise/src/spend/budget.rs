use crate::spend::error::Error;
use crate::spend::microdollars::Microdollars;
use crate::spend::state::State;

pub const DEFAULT_THRESHOLD: u8 = 80;

const PERCENT: i128 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Budget {
    pub limit: Microdollars,
    pub threshold_percent: u8,
    pub hard: bool,
}

impl Budget {
    pub(crate) fn checked(limit: i64, threshold_percent: i64, hard: bool) -> Result<Self, Error> {
        if limit <= 0 {
            return Err(Error::LimitInvalid);
        }
        let threshold_percent = u8::try_from(threshold_percent)
            .ok()
            .filter(|percent| (1..=100).contains(percent))
            .ok_or(Error::ThresholdInvalid)?;
        Ok(Self {
            limit: Microdollars(limit),
            threshold_percent,
            hard,
        })
    }

    pub fn state(&self, spent: Microdollars) -> State {
        if spent >= self.limit {
            return State::Exhausted;
        }
        let spent = i128::from(spent.0) * PERCENT;
        let threshold = i128::from(self.limit.0) * i128::from(self.threshold_percent);
        if spent >= threshold {
            State::Warning
        } else {
            State::Under
        }
    }
}

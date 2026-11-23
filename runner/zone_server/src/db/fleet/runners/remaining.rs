/// The connected runners that are active and current before and after a prospective change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Remaining {
    pub before: i64,
    pub after: i64,
}

impl Remaining {
    /// The change takes the last runner able to take work out of the fleet. A fleet already left
    /// with none, or a change to a runner that isn't counted, strands nothing more.
    pub fn strands(self) -> bool {
        self.before > 0 && self.after == 0
    }
}

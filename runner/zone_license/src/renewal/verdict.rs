use crate::renewal::reason::Reason;
use crate::terms::Terms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Renew(Terms),
    Current,
    Settling,
    Ended(Reason),
    Misconfigured,
}

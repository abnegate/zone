use abnegate_comfy::lora::Dropped;
use abnegate_comfy::lora::Remediation;
use abnegate_comfy::lora::Screening;
use serde::Serialize;

/// What a run kept of its set, what it dropped and which images it tried to
/// repair, as the client reads it for every subject.
#[derive(Clone, Debug, Serialize)]
pub struct ScreeningView {
    pub kept: usize,
    pub dropped: Vec<Dropped>,
    pub attempted: Vec<Remediation>,
}

impl ScreeningView {
    /// A set that kept all of its `kept` items, as a language run reports.
    pub fn kept(kept: usize) -> Self {
        Self {
            kept,
            dropped: Vec::new(),
            attempted: Vec::new(),
        }
    }
}

impl From<Screening> for ScreeningView {
    fn from(screening: Screening) -> Self {
        Self {
            kept: screening.kept,
            dropped: screening.dropped,
            attempted: screening.attempted,
        }
    }
}

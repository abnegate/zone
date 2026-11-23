use crate::bundle::Presigner;
use crate::gate::{Gate, Throttle};

#[derive(Debug, Clone)]
pub struct Distributor {
    pub(crate) gate: Gate,
    pub(crate) presigner: Presigner,
    pub(crate) throttle: Throttle,
}

impl Distributor {
    pub fn new(gate: Gate, presigner: Presigner, throttle: Throttle) -> Self {
        Self {
            gate,
            presigner,
            throttle,
        }
    }
}

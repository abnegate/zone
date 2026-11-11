//! What the workspace and the person add to a chat's system entry, read before
//! any of it is rendered, so the three blocks can share what the window
//! leaves.

use super::allowance::Allowance;
use super::fitted::Fitted;
use crate::agent::learned::{self, Learned};
use crate::agent::memory::render::Remembered;
use crate::agent::skills::Listing;

/// Everything a chat's optional blocks are rendered from. A block the chat
/// does not read is left at its default, which renders nothing and so takes
/// nothing from the others.
#[derive(Debug, Clone, Default)]
pub struct Blocks {
    pub learned: Learned,
    pub memory: Remembered,
    pub skills: Listing,
}

impl Blocks {
    /// The three blocks within `room` bytes between them, shared by
    /// [`Allowance::share`].
    ///
    /// Each is rendered at its ceiling first, which is what a block keeps
    /// when the room holds it whole and what tells the share how much each
    /// would take. A block left short renders again against its part, in the
    /// order the prompt reads them, and a part it does not spend, because it
    /// trims whole entries or cannot fit even its notice, is added to the
    /// part of the next short block rather than left unused.
    pub fn fit(&self, room: Option<usize>) -> Fitted {
        let ceilings = Allowance::CEILINGS;
        let whole = Fitted {
            findings: learned::render(&self.learned, ceilings.learned),
            memory: self.memory.render(ceilings.memory),
            skills: self.skills.render(ceilings.skills),
        };
        let allowance = Allowance::share(
            room,
            Allowance {
                learned: whole.findings.text.len(),
                memory: whole.memory.len(),
                skills: whole.skills.len(),
            },
        );
        let mut unspent = 0;
        let findings = if allowance.learned < ceilings.learned {
            let block = learned::render(&self.learned, allowance.learned + unspent);
            unspent = allowance.learned + unspent - block.text.len();
            block
        } else {
            whole.findings
        };
        let memory = if allowance.memory < ceilings.memory {
            let block = self.memory.render(allowance.memory + unspent);
            unspent = allowance.memory + unspent - block.len();
            block
        } else {
            whole.memory
        };
        let skills = if allowance.skills < ceilings.skills {
            self.skills.render(allowance.skills + unspent)
        } else {
            whole.skills
        };
        Fitted {
            findings,
            memory,
            skills,
        }
    }
}

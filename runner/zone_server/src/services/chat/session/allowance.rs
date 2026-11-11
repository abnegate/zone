//! How much of a chat's window each optional block of its system entry may
//! take.
//!
//! After the built prompt and the capability tail, the system entry carries
//! three blocks a turn can do without: what the workspace has learned, what is
//! remembered about the person, and the skills index. Each has a ceiling of
//! its own, and a large window carries all three at their ceilings. A small
//! one cannot: the system entry is preserved through every compaction, so
//! bytes it spends on these blocks are bytes no compaction can free, and a turn
//! whose preserved entries outgrow the window fails to prepare.
//!
//! So the three share what the window leaves once everything else a turn
//! cannot shed is counted ([`Allowance::room`]), and a block that cannot fit
//! even its own omission notice in its share is left out rather than sent
//! over.

use abnegate_agent::context::{self, Coverage, Entry, Policy, Summary};
use abnegate_llm::ToolDefinition;

use crate::agent::{learned, memory, skills};

/// What the context estimate counts as one token (`context::tokens`).
const BYTES_PER_TOKEN: u64 = 4;

/// The bytes each optional block may render to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowance {
    pub learned: usize,
    pub memory: usize,
    pub skills: usize,
}

impl Allowance {
    /// Every block at its own ceiling: what a window large enough to carry
    /// them all, or one of unknown size, renders.
    pub const CEILINGS: Self = Self {
        learned: learned::MAX_RENDERED_BYTES,
        memory: memory::render::MAX_RENDERED_BYTES,
        skills: skills::MAX_RENDERED_BYTES,
    };

    /// The bytes the three blocks may take between them in a turn whose
    /// `entries` are built with every block empty and which sends `tools`.
    /// `None` when the window's size is unknown, which leaves nothing to
    /// share.
    ///
    /// The line is the threshold `context::prepare` compacts at, less one
    /// token, because it compacts a turn that reaches it. Below that line the
    /// turn must keep what no compaction frees: every entry it preserves (the
    /// system entry as built, the question, the search supplement), the tool
    /// schemas it sends, and the checkpoint a compaction would write in place
    /// of the rest, counted as its framing plus the reply reservation, which
    /// `context::prepare` holds a checkpoint's content to. What is left, at
    /// four bytes a token, is the blocks'. So once a turn's history has to be
    /// compacted, the blocks never leave the checkpoint without the room it
    /// needs, and a turn that prepares without them prepares with them.
    pub fn room(
        model: &str,
        entries: &[Entry],
        tools: Option<&[ToolDefinition]>,
        policy: &Policy,
    ) -> Option<usize> {
        let line = policy.threshold()?.saturating_sub(1);
        let preserved: Vec<Entry> = entries
            .iter()
            .filter(|entry| entry.preserve)
            .cloned()
            .collect();
        let checkpoint = Summary::new(String::new(), Coverage::new(Vec::new(), String::new()), 1);
        let committed = context::estimate(model, &preserved, tools, policy, Some(&checkpoint))
            .used
            .saturating_add(u64::from(policy.reserved));
        let free = line.saturating_sub(committed);
        Some(usize::try_from(free.saturating_mul(BYTES_PER_TOKEN)).unwrap_or(usize::MAX))
    }

    /// `room` bytes shared between blocks that render `demand` bytes at their
    /// ceilings.
    ///
    /// When the room holds every block whole, each keeps its ceiling and
    /// renders exactly as it would with no window to fit. Otherwise the room
    /// is divided in proportion to the ceilings, the figures each block was
    /// already given as its weight beside the others; a block whose whole
    /// rendering fits its part keeps its ceiling and hands what it did not
    /// need back to be divided again among the rest, and the blocks that are
    /// still short render against their parts. An empty block takes nothing.
    pub fn share(room: Option<usize>, demand: Self) -> Self {
        let Some(room) = room else {
            return Self::CEILINGS;
        };
        let ceilings = Self::CEILINGS.parts();
        let demands = demand.parts();
        let mut budgets = ceilings;
        let mut left = room.min(demands.iter().sum());
        let mut open: Vec<usize> = (0..demands.len())
            .filter(|&block| demands[block] > 0)
            .collect();
        while !open.is_empty() {
            let weight: usize = open.iter().map(|&block| ceilings[block]).sum();
            let part = |block: usize| left * ceilings[block] / weight;
            let (whole, short): (Vec<usize>, Vec<usize>) = open
                .iter()
                .partition(|&&block| demands[block] <= part(block));
            if whole.is_empty() {
                for block in short {
                    budgets[block] = part(block);
                }
                break;
            }
            left -= whole.iter().map(|&block| demands[block]).sum::<usize>();
            open = short;
        }
        Self::from_parts(budgets)
    }

    fn parts(self) -> [usize; 3] {
        [self.learned, self.memory, self.skills]
    }

    fn from_parts([learned, memory, skills]: [usize; 3]) -> Self {
        Self {
            learned,
            memory,
            skills,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_agent::context::ContextSource;
    use abnegate_llm::Message;

    fn demand(learned: usize, memory: usize, skills: usize) -> Allowance {
        Allowance {
            learned,
            memory,
            skills,
        }
    }

    #[test]
    fn a_window_of_unknown_size_gives_every_block_its_ceiling() {
        assert_eq!(
            Allowance::share(None, demand(8_000, 6_000, 8_000)),
            Allowance::CEILINGS
        );
    }

    #[test]
    fn room_for_every_whole_block_gives_every_block_its_ceiling() {
        assert_eq!(
            Allowance::share(Some(22_000), demand(8_000, 6_000, 8_000)),
            Allowance::CEILINGS
        );
        assert_eq!(
            Allowance::share(Some(900), demand(300, 400, 200)),
            Allowance::CEILINGS
        );
    }

    /// 2,200 bytes divide 800, 600 and 800 by the ceilings' 8:6:8.
    #[test]
    fn three_full_blocks_divide_the_room_by_their_ceilings() {
        assert_eq!(
            Allowance::share(Some(2_200), demand(8_000, 6_000, 8_000)),
            demand(800, 600, 800)
        );
    }

    /// The memory block needs 100 of its 600, so the other 500 go back and
    /// the two still short divide 2,100 between them, 1,050 each.
    #[test]
    fn what_a_small_block_does_not_need_goes_to_the_others() {
        assert_eq!(
            Allowance::share(Some(2_200), demand(8_000, 100, 8_000)),
            demand(1_050, memory::render::MAX_RENDERED_BYTES, 1_050)
        );
    }

    /// A plain chat reads no skills index, so the learned and memory blocks
    /// divide everything by 8:6.
    #[test]
    fn an_empty_block_takes_nothing() {
        assert_eq!(
            Allowance::share(Some(1_400), demand(8_000, 6_000, 0)),
            demand(800, 600, skills::MAX_RENDERED_BYTES)
        );
    }

    #[test]
    fn no_room_gives_every_block_that_has_text_nothing() {
        assert_eq!(
            Allowance::share(Some(0), demand(8_000, 6_000, 0)),
            demand(0, 0, skills::MAX_RENDERED_BYTES)
        );
    }

    /// A 4,096-token window reserves 1,024 tokens for the reply, compacts at
    /// 2,458 of the 3,072 it leaves, and keeps 1,024 more for the checkpoint
    /// and that checkpoint's framing: a turn of a hundred-token system entry
    /// and nothing else leaves the blocks what remains, four bytes a token.
    #[test]
    fn the_room_is_what_the_compaction_line_leaves_after_everything_a_turn_keeps() {
        let policy = Policy::new(Some(4_096), 1_024, ContextSource::Runtime);
        let system = "s".repeat(400);
        let entries = [Entry::new("instructions", Message::system(system.as_str()))
            .with_preserve(true)
            .with_consumed(true)];
        let checkpoint = Summary::new(String::new(), Coverage::new(Vec::new(), String::new()), 1);
        let committed = context::estimate("model", &entries, None, &policy, Some(&checkpoint)).used;
        let expected = (2_458 - 1 - 1_024 - committed) * BYTES_PER_TOKEN;

        assert_eq!(
            Allowance::room("model", &entries, None, &policy),
            Some(usize::try_from(expected).unwrap())
        );
        assert!(committed > 100, "{committed} counts the framing too");

        let history = Entry::new("earlier", Message::user("x".repeat(40_000))).with_consumed(true);
        assert_eq!(
            Allowance::room("model", &[entries[0].clone(), history], None, &policy),
            Some(usize::try_from(expected).unwrap()),
            "history a compaction can fold away is not the blocks' to pay for"
        );
        assert_eq!(
            Allowance::room(
                "model",
                &entries,
                None,
                &Policy::new(None, 1_024, ContextSource::Unknown)
            ),
            None
        );
        let crowded =
            [Entry::new("instructions", Message::system("s".repeat(12_000))).with_preserve(true)];
        assert_eq!(Allowance::room("model", &crowded, None, &policy), Some(0));
    }
}

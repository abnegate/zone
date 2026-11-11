//! What a workspace has learned, bounded for a chat's system prompt.
//!
//! Three families are filed as a workspace's own findings: the standing
//! instructions promoted from answers it kept giving, the facts a learning
//! pass read out of its runs, and what its agents recorded in the middle of
//! their work. Each keeps its renderer beside its store in `db::knowledge`,
//! and this module renders nothing itself. What lives here is the one thing no
//! single family can own: the bound across all of them.
//!
//! A task run appends the same families held only by each one's own limit
//! (`workers::task`). A chat cannot spend that much: its system entry is
//! preserved through every compaction and sent again with every turn, so the
//! whole block is held to [`MAX_RENDERED_BYTES`], the figure the skills index
//! is held to. Bytes, because the prompt ceilings are `String::len()` and the
//! context estimate is byte-length based.
//!
//! Past the bound, whichever family currently renders largest gives up its
//! oldest entry, whole, and that repeats until the block fits. So no family is
//! emptied to spare another, nothing is cut mid-entry, and a family left with
//! no entry loses its heading too. The block then closes by saying how many
//! entries were left out, because a list that reads as complete and is not
//! would have the reader take a missing entry for no entry.
//!
//! The discoveries block arrives already held to a bound of its own and says
//! what that bound left out. The closing line here counts only what this bound
//! took on top, so the two never count one entry twice.
//!
//! The block opens with the blank line every block in a system prompt owns and
//! ends without a newline, so whatever follows it sits one blank line below.

use sqlx::PgPool;
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::knowledge::{self, LearnedCategory, LearnedEntryRow, discovery};

/// The whole block, every family and the closing line included, never exceeds
/// this.
pub const MAX_RENDERED_BYTES: usize = 8_000;

/// Everything a workspace has learned, each family most recent first.
#[derive(Debug, Clone, Default)]
pub struct Learned {
    pub instructions: Vec<LearnedEntryRow>,
    pub facts: Vec<(LearnedCategory, Vec<LearnedEntryRow>)>,
    pub discoveries: Vec<LearnedEntryRow>,
}

#[derive(Debug, Clone, Copy)]
enum Family {
    Instructions,
    Facts(LearnedCategory),
    Discoveries,
}

impl Family {
    fn render(self, entries: &[LearnedEntryRow]) -> String {
        match self {
            Family::Instructions => knowledge::render_standing_instructions(entries),
            Family::Facts(category) => knowledge::render_learned_facts(category, entries),
            Family::Discoveries => discovery::render(entries),
        }
    }
}

/// One family's block, and how many of its entries the block was given.
struct Section<'a> {
    family: Family,
    entries: &'a [LearnedEntryRow],
    shown: usize,
    block: String,
}

impl<'a> Section<'a> {
    fn new(family: Family, entries: &'a [LearnedEntryRow]) -> Self {
        Self {
            family,
            entries,
            shown: entries.len(),
            block: family.render(entries),
        }
    }

    fn withheld(&self) -> usize {
        self.entries.len() - self.shown
    }

    fn withhold_oldest(&mut self) {
        self.shown -= 1;
        self.block = self.family.render(&self.entries[..self.shown]);
    }
}

impl Learned {
    fn sections(&self) -> Vec<Section<'_>> {
        let mut sections = Vec::with_capacity(self.facts.len() + 2);
        sections.push(Section::new(Family::Instructions, &self.instructions));
        for (category, facts) in &self.facts {
            sections.push(Section::new(Family::Facts(*category), facts));
        }
        sections.push(Section::new(Family::Discoveries, &self.discoveries));
        sections
    }
}

fn omitted(count: usize) -> String {
    let entries = if count == 1 {
        "older learned entry is"
    } else {
        "older learned entries are"
    };
    format!("\n\n{count} {entries} filed in this workspace but not shown here.\n")
}

/// Appends one block a blank line below what is already there.
///
/// Every renderer opens its block with a blank line and closes it with a
/// newline, so what has been joined so far gives up its closing newline to the
/// block that follows. `db::knowledge` joins the learned facts the same way,
/// which is what keeps a chat and a run reading the same bytes.
fn push(block: &mut String, next: &str) {
    if next.is_empty() {
        return;
    }
    block.truncate(block.trim_end().len());
    block.push_str(next);
}

fn assemble(sections: &[Section<'_>]) -> String {
    let mut block = String::new();
    for section in sections {
        push(&mut block, &section.block);
    }
    let withheld: usize = sections.iter().map(Section::withheld).sum();
    if withheld > 0 {
        push(&mut block, &omitted(withheld));
    }
    block.truncate(block.trim_end().len());
    block
}

/// The block, ready to append to a system prompt. Empty when the workspace
/// has learned nothing, so a chat in one reads exactly as it did before.
pub fn render(learned: &Learned) -> String {
    let mut sections = learned.sections();
    let mut block = assemble(&sections);
    while block.len() > MAX_RENDERED_BYTES {
        let Some(largest) = sections
            .iter_mut()
            .filter(|section| section.shown > 0)
            .max_by_key(|section| section.block.len())
        else {
            break;
        };
        largest.withhold_oldest();
        block = assemble(&sections);
    }
    block
}

/// Every learned family of one workspace, read with the stores' own row
/// limits.
pub async fn load(pool: &PgPool, workspace_id: Uuid) -> DbResult<Learned> {
    let instructions = knowledge::list_standing_instructions(pool, workspace_id).await?;
    let mut facts = Vec::with_capacity(LearnedCategory::ALL.len());
    for category in LearnedCategory::ALL {
        facts.push((
            category,
            knowledge::list_learned_facts(pool, workspace_id, category).await?,
        ));
    }
    let discoveries = discovery::list(pool, workspace_id).await?;
    Ok(Learned {
        instructions,
        facts,
        discoveries,
    })
}

/// What a workspace has learned, rendered for a chat in it.
pub async fn prompt(pool: &PgPool, workspace_id: Uuid) -> DbResult<String> {
    Ok(render(&load(pool, workspace_id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::knowledge::{LearningProvenance, PromotionProvenance};
    use chrono::NaiveDate;

    const RUN: Uuid = Uuid::from_u128(0x0d15_c0de);

    fn confirmed() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 11).expect("a real date")
    }

    fn row(title: String, content: String, tags: Vec<String>) -> LearnedEntryRow {
        LearnedEntryRow {
            id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            title,
            content,
            tags,
            updated_at: None,
        }
    }

    /// Text of exactly `bytes`, opening with a marker no other entry shares.
    fn text(marker: &str, bytes: usize) -> String {
        let mut text = format!("{marker} ");
        text.push_str(&"x".repeat(bytes - text.len()));
        assert_eq!(text.len(), bytes);
        text
    }

    fn instruction(index: usize, bytes: usize) -> LearnedEntryRow {
        row(
            format!("Instruction {index:02}"),
            text(&format!("instruction-{index:02}"), bytes),
            PromotionProvenance {
                fingerprint: format!("promoted-{index:02}"),
                occurrences: 4,
                distinct_chats: 3,
                last_confirmed: confirmed(),
            }
            .tags(),
        )
    }

    fn fact(category: LearnedCategory, index: usize, bytes: usize) -> LearnedEntryRow {
        row(
            format!("Fact {index:02}"),
            text(&format!("{category}-{index:02}"), bytes),
            LearningProvenance {
                fingerprint: format!("learned-{index:02}"),
                observations: 9,
                distinct_runs: 4,
                confidence: 0.82,
                last_confirmed: confirmed(),
            }
            .tags(category),
        )
    }

    fn recorded(index: usize, bytes: usize) -> LearnedEntryRow {
        let subject = format!("Subject {index:02}");
        let tags = discovery::Provenance {
            kind: discovery::Kind::Lesson,
            source: discovery::Source::Run(RUN),
            last_confirmed: confirmed(),
        }
        .tags(&discovery::fingerprint(&subject));
        row(subject, text(&format!("discovery-{index:02}"), bytes), tags)
    }

    fn learned(
        instructions: Vec<LearnedEntryRow>,
        conventions: Vec<LearnedEntryRow>,
        lessons: Vec<LearnedEntryRow>,
        discoveries: Vec<LearnedEntryRow>,
    ) -> Learned {
        Learned {
            instructions,
            facts: vec![
                (LearnedCategory::RepositoryConvention, conventions),
                (LearnedCategory::StrategyLesson, lessons),
            ],
            discoveries,
        }
    }

    /// How many entries of each family a block shows, in the order the block
    /// renders them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Shown {
        instructions: usize,
        conventions: usize,
        lessons: usize,
        discoveries: usize,
    }

    impl Shown {
        fn of(block: &str) -> Self {
            Self {
                instructions: block.matches("\ninstruction-").count(),
                conventions: block.matches("\n- repository-convention-").count(),
                lessons: block.matches("\n- strategy-lesson-").count(),
                discoveries: block.matches(": discovery-").count(),
            }
        }

        fn total(self) -> usize {
            self.instructions + self.conventions + self.lessons + self.discoveries
        }
    }

    /// The block the store's own renderers produce for the first `shown` of
    /// each family, one blank line apart, closing on `notice`. Written without
    /// the module's own join so it cannot agree with it by construction.
    fn expected(learned: &Learned, shown: Shown, notice: &str) -> String {
        let blocks = [
            knowledge::render_standing_instructions(&learned.instructions[..shown.instructions]),
            knowledge::render_learned_facts(
                LearnedCategory::RepositoryConvention,
                &learned.facts[0].1[..shown.conventions],
            ),
            knowledge::render_learned_facts(
                LearnedCategory::StrategyLesson,
                &learned.facts[1].1[..shown.lessons],
            ),
            discovery::render(&learned.discoveries[..shown.discoveries]),
        ];
        let mut expected: String = blocks.iter().map(|block| block.trim_end()).collect();
        expected.push_str(notice);
        expected
    }

    fn entries(learned: &Learned) -> usize {
        learned.instructions.len()
            + learned
                .facts
                .iter()
                .map(|(_, facts)| facts.len())
                .sum::<usize>()
            + learned.discoveries.len()
    }

    #[test]
    fn the_bound_is_the_one_the_skills_index_is_held_to() {
        assert_eq!(MAX_RENDERED_BYTES, 8_000);
        assert_eq!(MAX_RENDERED_BYTES, crate::agent::skills::MAX_RENDERED_BYTES);
    }

    #[test]
    fn a_workspace_that_has_learned_nothing_pays_nothing() {
        assert_eq!(render(&Learned::default()), "");
        assert_eq!(
            render(&learned(Vec::new(), Vec::new(), Vec::new(), Vec::new())),
            ""
        );
    }

    #[test]
    fn under_the_bound_the_block_is_the_renderers_output_one_blank_line_apart() {
        let learned = learned(
            vec![instruction(0, 80)],
            vec![
                fact(LearnedCategory::RepositoryConvention, 0, 60),
                fact(LearnedCategory::RepositoryConvention, 1, 60),
            ],
            vec![fact(LearnedCategory::StrategyLesson, 0, 60)],
            vec![recorded(0, 60)],
        );
        let block = render(&learned);

        assert_eq!(
            block,
            format!(
                "{}{}{}{}",
                knowledge::render_standing_instructions(&learned.instructions).trim_end(),
                knowledge::render_learned_facts(
                    LearnedCategory::RepositoryConvention,
                    &learned.facts[0].1
                )
                .trim_end(),
                knowledge::render_learned_facts(
                    LearnedCategory::StrategyLesson,
                    &learned.facts[1].1
                )
                .trim_end(),
                discovery::render(&learned.discoveries).trim_end(),
            )
        );
        assert!(
            block.starts_with("\n\n# Standing instructions\n"),
            "{block}"
        );
        let headings = [
            "\n\n# Standing instructions\n",
            "\n\n# Repository conventions\n",
            "\n\n# What has worked here\n",
            "\n\n# Recorded while working here\n",
        ];
        let positions: Vec<usize> = headings
            .iter()
            .map(|heading| {
                block
                    .find(heading)
                    .unwrap_or_else(|| panic!("{heading:?} is missing from {block}"))
            })
            .collect();
        assert!(positions.is_sorted(), "{positions:?}: {block}");
        assert!(
            block.ends_with("(lesson, recorded during a task run on 2026-11-11)"),
            "{block}"
        );
        assert!(!block.contains("\n\n\n"), "{block}");
        assert!(!block.contains("not shown"), "{block}");
    }

    #[test]
    fn one_family_alone_renders_as_its_own_renderer_wrote_it() {
        let alone = learned(
            Vec::new(),
            Vec::new(),
            vec![fact(LearnedCategory::StrategyLesson, 0, 60)],
            Vec::new(),
        );
        let block = render(&alone);
        assert_eq!(
            block,
            knowledge::render_learned_facts(LearnedCategory::StrategyLesson, &alone.facts[1].1)
                .trim_end()
        );
        assert!(block.starts_with("\n\n# What has worked here\n"), "{block}");
        assert!(!block.ends_with('\n'), "{block}");
    }

    #[test]
    fn past_the_bound_the_largest_family_gives_up_its_oldest_entries_whole() {
        let learned = learned(
            (0..12).map(|index| instruction(index, 600)).collect(),
            (0..3)
                .map(|index| fact(LearnedCategory::RepositoryConvention, index, 80))
                .collect(),
            Vec::new(),
            (0..2).map(|index| recorded(index, 80)).collect(),
        );
        let block = render(&learned);
        let shown = Shown::of(&block);

        assert!(block.len() <= MAX_RENDERED_BYTES, "{} bytes", block.len());
        assert_eq!(
            (shown.conventions, shown.lessons, shown.discoveries),
            (3, 0, 2),
            "the smaller families are untouched: {block}"
        );
        assert!(
            shown.instructions > 0 && shown.instructions < 12,
            "{shown:?}"
        );
        let withheld = entries(&learned) - shown.total();
        assert_eq!(
            block,
            expected(
                &learned,
                shown,
                &format!(
                    "\n\n{withheld} older learned entries are filed in this workspace but not \
                     shown here."
                )
            ),
            "what is kept is the front of each family, every entry whole"
        );
        for (index, instruction) in learned.instructions.iter().enumerate() {
            assert_eq!(
                block.contains(&instruction.content),
                index < shown.instructions,
                "the rows arrive most recent first, so what goes is the tail: {}",
                instruction.title
            );
        }
        let one_more = Shown {
            instructions: shown.instructions + 1,
            ..shown
        };
        assert!(
            expected(
                &learned,
                one_more,
                &omitted(entries(&learned) - one_more.total())
            )
            .trim_end()
            .len()
                > MAX_RENDERED_BYTES,
            "nothing is left out that would have fitted"
        );
        assert!(block.starts_with("\n\n# Standing instructions\n"));
        assert!(!block.ends_with('\n'));
        assert!(!block.contains("\n\n\n"));
    }

    #[test]
    fn no_family_is_emptied_to_spare_another() {
        let learned = learned(
            (0..10).map(|index| instruction(index, 500)).collect(),
            (0..30)
                .map(|index| fact(LearnedCategory::RepositoryConvention, index, 200))
                .collect(),
            (0..2)
                .map(|index| fact(LearnedCategory::StrategyLesson, index, 80))
                .collect(),
            Vec::new(),
        );
        let block = render(&learned);
        let shown = Shown::of(&block);

        assert!(block.len() <= MAX_RENDERED_BYTES, "{} bytes", block.len());
        assert!(
            shown.instructions > 0 && shown.instructions < 10,
            "{shown:?}"
        );
        assert!(shown.conventions > 0 && shown.conventions < 30, "{shown:?}");
        assert_eq!(shown.lessons, 2, "{shown:?}");
        let withheld = entries(&learned) - shown.total();
        assert_eq!(
            block,
            expected(
                &learned,
                shown,
                &format!(
                    "\n\n{withheld} older learned entries are filed in this workspace but not \
                     shown here."
                )
            )
        );
    }

    #[test]
    fn one_entry_left_out_is_counted_in_the_singular() {
        let learned = learned(
            (0..2).map(|index| instruction(index, 3_900)).collect(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let block = render(&learned);

        assert!(block.len() <= MAX_RENDERED_BYTES, "{} bytes", block.len());
        assert!(block.contains(&learned.instructions[0].content));
        assert!(!block.contains("instruction-01"), "{block}");
        assert!(
            block.ends_with(
                "last confirmed 2026-11-11)\n\n1 older learned entry is filed in this workspace \
                 but not shown here."
            ),
            "{}",
            &block[block.len() - 160..]
        );
    }

    #[test]
    fn an_entry_larger_than_the_bound_is_left_out_rather_than_cut() {
        let block = render(&learned(
            vec![instruction(0, MAX_RENDERED_BYTES)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            block,
            "\n\n1 older learned entry is filed in this workspace but not shown here."
        );
    }

    /// Forty discoveries of the most one may hold render five and say thirty
    /// five were left out, which is the discoveries block's own bound at work.
    /// Under this bound that block arrives as it is.
    #[test]
    fn what_the_discoveries_own_bound_left_out_stays_in_its_own_words() {
        let learned = learned(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            (0..40)
                .map(|index| recorded(index, discovery::MAX_ENTRY_BYTES - "Subject 00".len()))
                .collect(),
        );
        let block = render(&learned);

        assert_eq!(block, discovery::render(&learned.discoveries).trim_end());
        assert_eq!(Shown::of(&block).discoveries, 5);
        assert!(
            block.ends_with("\n\n35 older entries are recorded here but not shown."),
            "{}",
            &block[block.len() - 120..]
        );
        assert!(!block.contains("older learned"), "{block}");
    }

    /// The same forty beside enough standing instructions to push the whole
    /// past this bound: the discoveries are the larger family, so they give
    /// first, and every entry either bound left out is counted exactly once.
    #[test]
    fn an_entry_left_out_by_either_bound_is_counted_once() {
        let learned = learned(
            (0..4).map(|index| instruction(index, 600)).collect(),
            Vec::new(),
            Vec::new(),
            (0..40)
                .map(|index| recorded(index, discovery::MAX_ENTRY_BYTES - "Subject 00".len()))
                .collect(),
        );
        let block = render(&learned);
        let shown = Shown::of(&block);

        assert!(block.len() <= MAX_RENDERED_BYTES, "{} bytes", block.len());
        assert_eq!(shown.instructions, 4, "{shown:?}");
        assert!(shown.discoveries > 0 && shown.discoveries < 5, "{shown:?}");
        assert!(
            !block.contains("recorded here but not shown"),
            "the discoveries block was handed only what it could show whole: {block}"
        );
        let withheld = 40 - shown.discoveries;
        assert_eq!(
            block,
            expected(
                &learned,
                shown,
                &format!(
                    "\n\n{withheld} older learned entries are filed in this workspace but not \
                     shown here."
                )
            )
        );
        assert_eq!(block.matches("not shown").count(), 1, "{block}");
    }
}

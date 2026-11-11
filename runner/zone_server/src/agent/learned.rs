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
//! Past the bound, an entry too large to be shown even alone under its
//! family's heading goes first, so the entries filed before it are not spent
//! making room it could never use. Then whichever family currently renders
//! largest gives up its oldest entry, whole, and that repeats until the block
//! fits. A family down to its last entry gives nothing while another still
//! has a second, so no family is emptied to spare another, nothing is cut
//! mid-entry, and a family left with no entry loses its heading too.
//!
//! The block then closes by saying how many entries were left out, because a
//! list that reads as complete and is not would have the reader take a missing
//! entry for no entry. The count includes what the stores' own row limits
//! never read, and with no entry shown at all the line stands under a heading
//! of its own rather than under whatever the prompt said last.
//!
//! The discoveries block arrives already held to a bound of its own and says
//! what that bound left out. The closing line here counts only what this bound
//! took on top, so the two never count one entry twice.
//!
//! The block opens with the blank line every block in a system prompt owns and
//! ends without a newline, so whatever follows it sits one blank line below.
//! It also names the entries it shows, so a turn that retrieves from the same
//! store can leave those out and still find the ones a bound withheld. Which
//! discoveries those are is their own block's answer, since it may pass over
//! one in the middle of what it was handed.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::future::Future;

use futures::future::join_all;
use sqlx::PgPool;
use uuid::Uuid;

use crate::agent::skills;
use crate::db::DbResult;
use crate::db::knowledge::{self, LearnedCategory, LearnedEntryRow, discovery};

/// The whole block, every family and the closing line included, never exceeds
/// this. The skills index's figure rather than a second one: both are
/// workspace text that a chat's system entry carries through every compaction
/// and sends again with every turn, so what either may add to a prompt is one
/// number, changed in one place.
pub const MAX_RENDERED_BYTES: usize = skills::MAX_RENDERED_BYTES;

const HEADING: &str = "Learned in this workspace";

/// One kind of learned fact and what is filed under it, most recent first.
#[derive(Debug, Clone)]
pub struct Facts {
    pub category: LearnedCategory,
    pub entries: Vec<LearnedEntryRow>,
}

/// Everything a workspace has learned, each family most recent first.
#[derive(Debug, Clone, Default)]
pub struct Learned {
    pub instructions: Vec<LearnedEntryRow>,
    pub facts: Vec<Facts>,
    pub discoveries: Vec<LearnedEntryRow>,
    /// How many standing instructions and learned facts are filed past their
    /// stores' row limits and so were never read. The discoveries block
    /// accounts for its own.
    pub unread: usize,
}

/// What a chat's system prompt carries of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Block {
    /// Ready to append to a system prompt. Empty when the workspace has
    /// learned nothing, so a chat in one reads exactly as it did before.
    pub text: String,
    /// The entries `text` shows. One a bound withheld is not among them.
    pub shown: HashSet<Uuid>,
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

    /// Whether `entry` can be shown at all: alone under its family's heading
    /// it is within the bound. A discovery always answers that it can, because
    /// its block leaves out one too large for it and counts it there.
    fn fits(self, entry: &LearnedEntryRow) -> bool {
        self.render(std::slice::from_ref(entry)).trim_end().len() <= MAX_RENDERED_BYTES
    }

    /// Which of `entries` the family's block for them shows. Every family
    /// shows all it is handed except the discoveries, whose block keeps to a
    /// bound of its own and says which rows that bound let through.
    fn shown(self, entries: &[LearnedEntryRow]) -> Vec<Uuid> {
        match self {
            Family::Instructions | Family::Facts(_) => {
                entries.iter().map(|entry| entry.id).collect()
            }
            Family::Discoveries => discovery::block(entries, entries.len()).shown,
        }
    }
}

/// One family's block, and what of the family the block was handed.
struct Section<'a> {
    family: Family,
    /// Most recent first. The family's own rows until one of them proves too
    /// large to show.
    entries: Cow<'a, [LearnedEntryRow]>,
    /// How many of `entries` the block was handed, from the front.
    handed: usize,
    withheld: usize,
    block: String,
}

impl<'a> Section<'a> {
    fn new(family: Family, entries: &'a [LearnedEntryRow]) -> Self {
        Self {
            family,
            entries: Cow::Borrowed(entries),
            handed: entries.len(),
            withheld: 0,
            block: family.render(entries),
        }
    }

    fn withhold_oversized(&mut self) {
        let family = self.family;
        if self.entries.iter().all(|entry| family.fits(entry)) {
            return;
        }
        let entries = self.entries.to_mut();
        let filed = entries.len();
        entries.retain(|entry| family.fits(entry));
        self.withheld += filed - entries.len();
        self.handed = entries.len();
        self.block = family.render(entries);
    }

    fn withhold_oldest(&mut self) {
        self.handed -= 1;
        self.withheld += 1;
        self.block = self.family.render(&self.entries[..self.handed]);
    }

    fn shown(&self) -> Vec<Uuid> {
        self.family.shown(&self.entries[..self.handed])
    }
}

impl Learned {
    fn sections(&self) -> Vec<Section<'_>> {
        let mut sections = Vec::with_capacity(self.facts.len() + 2);
        sections.push(Section::new(Family::Instructions, &self.instructions));
        for facts in &self.facts {
            sections.push(Section::new(Family::Facts(facts.category), &facts.entries));
        }
        sections.push(Section::new(Family::Discoveries, &self.discoveries));
        sections
    }
}

/// The closing line: `count` entries are filed and not shown. `alone` when no
/// family's block stands above it, which is when it needs a heading.
fn omitted(count: usize, alone: bool) -> String {
    let entries = if count == 1 {
        "learned entry is"
    } else {
        "learned entries are"
    };
    let opening = if alone {
        format!("# {HEADING}\n{count}")
    } else {
        format!("{count} more")
    };
    format!("\n\n{opening} {entries} filed in this workspace but not shown here.\n")
}

fn assemble(sections: &[Section<'_>], unread: usize) -> String {
    let mut blocks: Vec<&str> = sections
        .iter()
        .map(|section| section.block.as_str())
        .collect();
    let withheld = unread
        + sections
            .iter()
            .map(|section| section.withheld)
            .sum::<usize>();
    let notice = match withheld {
        0 => String::new(),
        count => omitted(count, blocks.iter().all(|block| block.is_empty())),
    };
    blocks.push(&notice);
    let mut text = knowledge::join_blocks(&blocks);
    text.truncate(text.trim_end().len());
    text
}

/// The family that gives up its oldest entry next: the one rendering largest
/// among those with an entry to spare, and only when none has a second entry,
/// the largest that has one at all.
fn yielding<'sections, 'entries>(
    sections: &'sections mut [Section<'entries>],
) -> Option<&'sections mut Section<'entries>> {
    let spared = usize::from(sections.iter().any(|section| section.handed > 1));
    sections
        .iter_mut()
        .filter(|section| section.handed > spared)
        .max_by_key(|section| section.block.len())
}

/// The block for a chat's system prompt, and the entries it shows.
pub fn render(learned: &Learned) -> Block {
    let mut sections = learned.sections();
    let mut text = assemble(&sections, learned.unread);
    if text.len() > MAX_RENDERED_BYTES {
        sections.iter_mut().for_each(Section::withhold_oversized);
        text = assemble(&sections, learned.unread);
    }
    while text.len() > MAX_RENDERED_BYTES {
        let Some(section) = yielding(&mut sections) else {
            break;
        };
        section.withhold_oldest();
        text = assemble(&sections, learned.unread);
    }
    let shown = sections.iter().flat_map(Section::shown).collect();
    Block { text, shown }
}

/// One family's rows, or none of them and a warning when its store cannot be
/// read: a failed family costs a chat that family, as it costs a run.
async fn read(
    workspace_id: Uuid,
    category: &str,
    rows: impl Future<Output = DbResult<Vec<LearnedEntryRow>>>,
) -> Vec<LearnedEntryRow> {
    rows.await.unwrap_or_else(|error| {
        tracing::warn!(
            %workspace_id,
            category,
            %error,
            "Failed to load a learned family; continuing without it"
        );
        Vec::new()
    })
}

/// How many active entries the workspace has filed under each category whose
/// store reads to a row limit and says nothing of the rest.
async fn tally(pool: &PgPool, workspace_id: Uuid) -> DbResult<HashMap<String, i64>> {
    let categories: Vec<&str> = std::iter::once(knowledge::STANDING_INSTRUCTION_CATEGORY)
        .chain(LearnedCategory::ALL.map(LearnedCategory::as_str))
        .collect();
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT category, count(*) FROM knowledge_entries \
         WHERE workspace_id = $1 AND category = ANY($2) AND is_active = TRUE \
         GROUP BY category",
    )
    .bind(workspace_id)
    .bind(categories)
    .fetch_all(pool)
    .await?;
    Ok(counts.into_iter().collect())
}

/// How many of a category's entries its store's row limit kept from being
/// read. A family that arrived empty was either never filed or could not be
/// read, and counts for nothing either way.
fn unread(filed: &HashMap<String, i64>, category: &str, entries: &[LearnedEntryRow]) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let filed = filed.get(category).copied().unwrap_or_default();
    usize::try_from(filed)
        .unwrap_or_default()
        .saturating_sub(entries.len())
}

/// Every learned family of one workspace, read together with the stores' own
/// row limits.
pub async fn load(pool: &PgPool, workspace_id: Uuid) -> Learned {
    let (instructions, facts, discoveries, filed) = tokio::join!(
        read(
            workspace_id,
            knowledge::STANDING_INSTRUCTION_CATEGORY,
            knowledge::list_standing_instructions(pool, workspace_id),
        ),
        join_all(LearnedCategory::ALL.map(|category| async move {
            Facts {
                category,
                entries: read(
                    workspace_id,
                    category.as_str(),
                    knowledge::list_learned_facts(pool, workspace_id, category),
                )
                .await,
            }
        })),
        read(
            workspace_id,
            discovery::CATEGORY,
            discovery::list(pool, workspace_id),
        ),
        tally(pool, workspace_id),
    );
    let filed = filed.unwrap_or_else(|error| {
        tracing::warn!(
            %workspace_id,
            %error,
            "Failed to count the learned entries filed; the closing line counts only what was read"
        );
        HashMap::new()
    });
    let past_limits = unread(
        &filed,
        knowledge::STANDING_INSTRUCTION_CATEGORY,
        &instructions,
    ) + facts
        .iter()
        .map(|facts| unread(&filed, facts.category.as_str(), &facts.entries))
        .sum::<usize>();
    Learned {
        instructions,
        facts,
        discoveries,
        unread: past_limits,
    }
}

/// What a workspace has learned, rendered for a chat in it. A store that
/// cannot be read costs the turn what it held and nothing else.
pub async fn prompt(pool: &PgPool, workspace_id: Uuid) -> Block {
    render(&load(pool, workspace_id).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::knowledge::{LearningProvenance, PromotionProvenance};
    use chrono::NaiveDate;
    use std::time::Duration;

    const RUN: Uuid = Uuid::from_u128(0x0d15_c0de);

    fn confirmed() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 11).expect("a real date")
    }

    fn row(title: String, content: String, tags: Vec<String>) -> LearnedEntryRow {
        LearnedEntryRow {
            id: Uuid::new_v4(),
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
                Facts {
                    category: LearnedCategory::RepositoryConvention,
                    entries: conventions,
                },
                Facts {
                    category: LearnedCategory::StrategyLesson,
                    entries: lessons,
                },
            ],
            discoveries,
            unread: 0,
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
                &learned.facts[0].entries[..shown.conventions],
            ),
            knowledge::render_learned_facts(
                LearnedCategory::StrategyLesson,
                &learned.facts[1].entries[..shown.lessons],
            ),
            discovery::render(&learned.discoveries[..shown.discoveries]),
        ];
        let mut expected: String = blocks.iter().map(|block| block.trim_end()).collect();
        expected.push_str(notice);
        expected
    }

    /// The closing line as a reader meets it, spelled out here so a reworded
    /// one fails a test rather than agreeing with itself.
    fn more(count: usize) -> String {
        match count {
            1 => "\n\n1 more learned entry is filed in this workspace but not shown here."
                .to_string(),
            count => format!(
                "\n\n{count} more learned entries are filed in this workspace but not shown here."
            ),
        }
    }

    fn entries(learned: &Learned) -> usize {
        learned.instructions.len()
            + learned
                .facts
                .iter()
                .map(|facts| facts.entries.len())
                .sum::<usize>()
            + learned.discoveries.len()
    }

    /// The ids of the first `shown` entries of each family.
    fn ids(learned: &Learned, shown: Shown) -> HashSet<Uuid> {
        learned.instructions[..shown.instructions]
            .iter()
            .chain(&learned.facts[0].entries[..shown.conventions])
            .chain(&learned.facts[1].entries[..shown.lessons])
            .chain(&learned.discoveries[..shown.discoveries])
            .map(|entry| entry.id)
            .collect()
    }

    fn unreachable() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy("postgres://127.0.0.1:1/zone")
            .expect("a lazy pool needs no server")
    }

    #[test]
    fn a_workspace_that_has_learned_nothing_pays_nothing() {
        assert_eq!(render(&Learned::default()), Block::default());
        assert_eq!(
            render(&learned(Vec::new(), Vec::new(), Vec::new(), Vec::new())),
            Block::default()
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
        let text = &block.text;

        assert_eq!(
            *text,
            format!(
                "{}{}{}{}",
                knowledge::render_standing_instructions(&learned.instructions).trim_end(),
                knowledge::render_learned_facts(
                    LearnedCategory::RepositoryConvention,
                    &learned.facts[0].entries
                )
                .trim_end(),
                knowledge::render_learned_facts(
                    LearnedCategory::StrategyLesson,
                    &learned.facts[1].entries
                )
                .trim_end(),
                discovery::render(&learned.discoveries).trim_end(),
            )
        );
        assert!(text.starts_with("\n\n# Standing instructions\n"), "{text}");
        let headings = [
            "\n\n# Standing instructions\n",
            "\n\n# Repository conventions\n",
            "\n\n# What has worked here\n",
            "\n\n# Recorded while working here\n",
        ];
        let positions: Vec<usize> = headings
            .iter()
            .map(|heading| {
                text.find(heading)
                    .unwrap_or_else(|| panic!("{heading:?} is missing from {text}"))
            })
            .collect();
        assert!(positions.is_sorted(), "{positions:?}: {text}");
        assert!(!text.ends_with('\n'), "{text}");
        assert!(!text.contains("\n\n\n"), "{text}");
        assert!(!text.contains("not shown"), "{text}");
        assert_eq!(block.shown, ids(&learned, Shown::of(text)));
        assert_eq!(block.shown.len(), 5);
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
            block.text,
            knowledge::render_learned_facts(
                LearnedCategory::StrategyLesson,
                &alone.facts[1].entries
            )
            .trim_end()
        );
        assert!(
            block.text.starts_with("\n\n# What has worked here\n"),
            "{}",
            block.text
        );
        assert!(!block.text.ends_with('\n'), "{}", block.text);
        assert_eq!(block.shown, HashSet::from([alone.facts[1].entries[0].id]));
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
        let text = &block.text;
        let shown = Shown::of(text);

        assert!(text.len() <= MAX_RENDERED_BYTES, "{} bytes", text.len());
        assert_eq!(
            (shown.conventions, shown.lessons, shown.discoveries),
            (3, 0, 2),
            "the smaller families are untouched: {text}"
        );
        assert!(
            shown.instructions > 0 && shown.instructions < 12,
            "{shown:?}"
        );
        let withheld = entries(&learned) - shown.total();
        assert_eq!(
            *text,
            expected(&learned, shown, &more(withheld)),
            "what is kept is the front of each family, every entry whole"
        );
        for (index, instruction) in learned.instructions.iter().enumerate() {
            assert_eq!(
                text.contains(&instruction.content),
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
                &more(entries(&learned) - one_more.total())
            )
            .len()
                > MAX_RENDERED_BYTES,
            "nothing is left out that would have fitted"
        );
        assert!(text.starts_with("\n\n# Standing instructions\n"));
        assert!(!text.ends_with('\n'));
        assert!(!text.contains("\n\n\n"));
        assert_eq!(block.shown, ids(&learned, shown));
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
        let shown = Shown::of(&block.text);

        assert!(
            block.text.len() <= MAX_RENDERED_BYTES,
            "{} bytes",
            block.text.len()
        );
        assert!(
            shown.instructions > 0 && shown.instructions < 10,
            "{shown:?}"
        );
        assert!(shown.conventions > 0 && shown.conventions < 30, "{shown:?}");
        assert_eq!(shown.lessons, 2, "{shown:?}");
        let withheld = entries(&learned) - shown.total();
        assert_eq!(block.text, expected(&learned, shown, &more(withheld)));
    }

    /// One long standing instruction is the largest block there is, and the
    /// only entry its family has. The conventions beside it have thirty: they
    /// give, and the instruction and its heading stay.
    #[test]
    fn a_family_of_one_keeps_its_entry_while_another_has_entries_to_spare() {
        let learned = learned(
            vec![instruction(0, 3_900)],
            (0..30)
                .map(|index| fact(LearnedCategory::RepositoryConvention, index, 200))
                .collect(),
            Vec::new(),
            Vec::new(),
        );
        let block = render(&learned);
        let text = &block.text;
        let shown = Shown::of(text);

        assert!(text.len() <= MAX_RENDERED_BYTES, "{} bytes", text.len());
        assert!(text.starts_with("\n\n# Standing instructions\n"), "{text}");
        assert!(text.contains(&learned.instructions[0].content), "{text}");
        assert_eq!(shown.instructions, 1, "{shown:?}");
        assert!(shown.conventions > 0 && shown.conventions < 30, "{shown:?}");
        assert_eq!(
            *text,
            expected(&learned, shown, &more(30 - shown.conventions))
        );
        let one_more = Shown {
            conventions: shown.conventions + 1,
            ..shown
        };
        assert!(
            expected(&learned, one_more, &more(30 - one_more.conventions)).len()
                > MAX_RENDERED_BYTES,
            "the conventions gave up no more than the bound asked of them"
        );
        assert_eq!(block.shown, ids(&learned, shown));
    }

    /// With every family down to one entry something has to go, and it is the
    /// largest. What went was the only standing instruction, so the closing
    /// line cannot call it older than anything.
    #[test]
    fn a_last_entry_goes_only_when_no_family_has_a_second() {
        let learned = learned(
            vec![instruction(0, 5_000)],
            vec![fact(LearnedCategory::RepositoryConvention, 0, 3_500)],
            Vec::new(),
            Vec::new(),
        );
        let block = render(&learned);
        let kept = Shown {
            instructions: 0,
            conventions: 1,
            lessons: 0,
            discoveries: 0,
        };

        assert_eq!(block.text, expected(&learned, kept, &more(1)));
        assert!(!block.text.contains("older"), "{}", block.text);
        assert!(
            !block.text.contains("# Standing instructions"),
            "{}",
            block.text
        );
        assert_eq!(block.shown, ids(&learned, kept));
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
        let kept = Shown {
            instructions: 1,
            conventions: 0,
            lessons: 0,
            discoveries: 0,
        };

        assert!(
            block.text.len() <= MAX_RENDERED_BYTES,
            "{} bytes",
            block.text.len()
        );
        assert_eq!(block.text, expected(&learned, kept, &more(1)));
        assert!(
            block.text.ends_with(
                ")\n\n1 more learned entry is filed in this workspace but not shown here."
            ),
            "{}",
            &block.text[block.text.len() - 160..]
        );
        assert_eq!(block.shown, HashSet::from([learned.instructions[0].id]));
    }

    /// Nothing can be shown, so the line has no family's block to stand
    /// under and brings a heading of its own. It says nothing of "more",
    /// because there is nothing it is more than.
    #[test]
    fn with_nothing_shown_the_closing_line_stands_under_its_own_heading() {
        let block = render(&learned(
            vec![instruction(0, MAX_RENDERED_BYTES)],
            vec![
                fact(LearnedCategory::RepositoryConvention, 0, MAX_RENDERED_BYTES),
                fact(LearnedCategory::RepositoryConvention, 1, MAX_RENDERED_BYTES),
            ],
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            block.text,
            "\n\n# Learned in this workspace\n3 learned entries are filed in this workspace but \
             not shown here."
        );
        assert_eq!(block.shown, HashSet::new());

        let one = render(&learned(
            vec![instruction(0, MAX_RENDERED_BYTES)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            one.text,
            "\n\n# Learned in this workspace\n1 learned entry is filed in this workspace but \
             not shown here."
        );
    }

    /// The newest instruction can never be shown. Removing oldest first would
    /// spend the two behind it before reaching it; they fit, so they stay.
    #[test]
    fn an_entry_larger_than_the_bound_goes_before_the_entries_that_fit() {
        let learned = learned(
            vec![
                instruction(0, MAX_RENDERED_BYTES),
                instruction(1, 300),
                instruction(2, 300),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let block = render(&learned);

        assert!(
            block.text.len() <= MAX_RENDERED_BYTES,
            "{} bytes",
            block.text.len()
        );
        assert_eq!(
            block.text,
            format!(
                "{}{}",
                knowledge::render_standing_instructions(&learned.instructions[1..]).trim_end(),
                more(1)
            )
        );
        assert!(!block.text.contains("instruction-00"), "{}", block.text);
        assert!(!block.text.contains("older"), "{}", block.text);
        assert_eq!(
            block.shown,
            HashSet::from([learned.instructions[1].id, learned.instructions[2].id])
        );
    }

    /// A block under the bound still closes on the line when a store's row
    /// limit kept entries from being read, and past the bound the two counts
    /// are one sum.
    #[test]
    fn entries_past_a_row_limit_are_counted_in_the_closing_line() {
        let mut under = learned(
            vec![instruction(0, 80)],
            vec![fact(LearnedCategory::RepositoryConvention, 0, 60)],
            Vec::new(),
            Vec::new(),
        );
        under.unread = 3;
        let all = Shown {
            instructions: 1,
            conventions: 1,
            lessons: 0,
            discoveries: 0,
        };
        assert_eq!(render(&under).text, expected(&under, all, &more(3)));

        let mut past = learned(
            (0..12).map(|index| instruction(index, 800)).collect(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        past.unread = 7;
        let block = render(&past);
        let shown = Shown::of(&block.text);
        assert!(
            shown.instructions > 0 && shown.instructions < 12,
            "{shown:?}"
        );
        assert_eq!(
            block.text,
            expected(&past, shown, &more(12 - shown.instructions + 7))
        );
        assert!(block.text.len() <= MAX_RENDERED_BYTES);
    }

    #[test]
    fn a_row_limit_is_counted_only_for_a_family_that_was_read() {
        let category = LearnedCategory::RepositoryConvention.as_str();
        let filed = HashMap::from([(category.to_string(), 45_i64)]);
        let read: Vec<LearnedEntryRow> = (0..40)
            .map(|index| fact(LearnedCategory::RepositoryConvention, index, 40))
            .collect();

        assert_eq!(unread(&filed, category, &read), 5);
        assert_eq!(unread(&filed, category, &read[..0]), 0);
        assert_eq!(
            unread(&filed, LearnedCategory::StrategyLesson.as_str(), &read),
            0
        );
        assert_eq!(
            unread(
                &HashMap::from([(category.to_string(), 39_i64)]),
                category,
                &read
            ),
            0,
            "a row retired between the two reads is not a negative count"
        );
    }

    /// Forty discoveries of the most one may hold are more than the
    /// discoveries block shows, which is its own bound at work and told in
    /// its own words. Under this bound that block arrives as it is, and only
    /// the entries it shows are named as shown.
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
        let shown = Shown::of(&block.text);

        assert_eq!(
            block.text,
            discovery::render(&learned.discoveries).trim_end()
        );
        assert!(shown.discoveries > 0 && shown.discoveries < 40, "{shown:?}");
        assert!(!block.text.contains("learned entr"), "{}", block.text);
        assert_eq!(block.shown, ids(&learned, shown));
        for (index, discovery) in learned.discoveries.iter().enumerate() {
            assert_eq!(
                block.shown.contains(&discovery.id),
                block.text.contains(&discovery.content),
                "discovery {index} is named as shown exactly when its text is"
            );
        }
    }

    /// The discoveries block passes over a row too large for it and shows the
    /// rows behind it, so what it shows is not the front of what it was
    /// handed. The row passed over is not named as shown, or a turn's
    /// retrieval would drop the one entry the chat has not read.
    #[test]
    fn a_discovery_its_block_passed_over_is_not_named_as_shown() {
        let learned = learned(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![
                recorded(0, 60),
                recorded(1, discovery::MAX_RENDERED_BYTES),
                recorded(2, 60),
            ],
        );
        let block = render(&learned);

        assert_eq!(
            block.text,
            discovery::render(&learned.discoveries).trim_end()
        );
        assert!(
            block.text.contains("discovery-00") && block.text.contains("discovery-02"),
            "{}",
            block.text
        );
        assert!(!block.text.contains("discovery-01"), "{}", block.text);
        assert!(
            block
                .text
                .ends_with("\n\n1 older entry is recorded here but not shown."),
            "{}",
            block.text
        );
        assert_eq!(
            block.shown,
            HashSet::from([learned.discoveries[0].id, learned.discoveries[2].id])
        );
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
        let shown = Shown::of(&block.text);

        assert!(
            block.text.len() <= MAX_RENDERED_BYTES,
            "{} bytes",
            block.text.len()
        );
        assert_eq!(shown.instructions, 4, "{shown:?}");
        assert!(shown.discoveries > 0, "{shown:?}");
        assert_eq!(
            Shown::of(&discovery::render(
                &learned.discoveries[..shown.discoveries]
            ))
            .discoveries,
            shown.discoveries,
            "the discoveries block was handed only what it could show whole, so it has nothing \
             of its own to count"
        );
        assert_eq!(
            block.text,
            expected(&learned, shown, &more(40 - shown.discoveries))
        );
        assert_eq!(block.text.matches("not shown").count(), 1, "{}", block.text);
        assert_eq!(block.shown, ids(&learned, shown));
    }

    #[tokio::test]
    async fn a_family_that_cannot_be_read_costs_only_itself() {
        let workspace = Uuid::new_v4();
        let conventions = vec![fact(LearnedCategory::RepositoryConvention, 0, 60)];

        let failed = read(workspace, knowledge::STANDING_INSTRUCTION_CATEGORY, async {
            Err(sqlx::Error::PoolTimedOut)
        })
        .await;
        let kept = read(
            workspace,
            LearnedCategory::RepositoryConvention.as_str(),
            async { Ok(conventions.clone()) },
        )
        .await;

        assert!(failed.is_empty(), "{failed:?}");
        let learned = learned(failed, kept, Vec::new(), Vec::new());
        assert_eq!(
            render(&learned).text,
            knowledge::render_learned_facts(LearnedCategory::RepositoryConvention, &conventions)
                .trim_end()
        );
    }

    /// A pool that can reach no server stands in for a store that is down:
    /// the chat is handed an empty block instead of an error.
    #[tokio::test]
    async fn an_unreachable_store_costs_a_chat_the_block_and_nothing_else() {
        let pool = unreachable();
        let workspace = Uuid::new_v4();
        assert!(
            knowledge::list_standing_instructions(&pool, workspace)
                .await
                .is_err(),
            "the store must be unreachable for this to prove anything"
        );

        let loaded = load(&pool, workspace).await;

        assert!(loaded.instructions.is_empty());
        assert_eq!(loaded.facts.len(), LearnedCategory::ALL.len());
        assert!(loaded.facts.iter().all(|facts| facts.entries.is_empty()));
        assert!(loaded.discoveries.is_empty());
        assert_eq!(loaded.unread, 0);
        assert_eq!(prompt(&pool, workspace).await, Block::default());
    }
}

//! What one person asked to have remembered.
//!
//! Every other category in `knowledge_entries` belongs to the workspace. These
//! three belong to whoever wrote them, and `created_by` is the owner: the
//! predicate `category NOT LIKE 'memory-%'` keeps them out of every
//! workspace-wide read, and every statement here scopes by `workspace_id`,
//! `created_by` and `category` together. A row whose owner has been deleted is
//! nobody's — `NULL = $2` is never true — so no read returns it and no write
//! touches it.
//!
//! Two spellings, on purpose. The stored form carries the prefix because that
//! prefix *is* the privacy discriminator; the short form is the only word the
//! model ever sees, which is why [`MemoryCategory`]'s `Display` renders it and
//! the message builders below cannot leak the other one.
//!
//! The builders name `memory_list` as text rather than as the constant that
//! defines it, because the tool names live in `agent::memory` and `db` does not
//! import `agent`.
//!
//! [`append`] reads, composes and writes from Rust rather than concatenating in
//! SQL, because the length limit counts characters and the refusal has to name
//! what it refused. Two appends that interleave lose nothing — each updates
//! against the version it read, so the later one is told it conflicted and
//! composes again from what it then reads — but the order the two additions end
//! up in is unspecified. That is why a version token is required of a write and
//! a delete and not of an append.

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::DbResult;
use super::knowledge::CHARACTERS_PER_TOKEN;

/// Every memory category starts with this, and the read-path predicate is
/// `category NOT LIKE 'memory-%'`. A category added outside this prefix would
/// be invisible to that predicate, so the prefix is pinned by a test.
pub const MEMORY_CATEGORY_PREFIX: &str = "memory-";
pub const PROFILE_CATEGORY: &str = "memory-profile";
pub const PREFERENCE_CATEGORY: &str = "memory-preference";
pub const FACT_CATEGORY: &str = "memory-fact";
pub const PROFILE_TITLE: &str = "Profile";
pub const PREFERENCES_TITLE: &str = "Preferences";

/// How long one entry may be, in characters.
///
/// The same bound as `agent::question::MAX_FREE_TEXT`, which is already this
/// codebase's answer to "how long is one thing a user types". Stated rather
/// than derived from it: `db` does not import `agent`.
pub const MAX_ENTRY_CHARS: usize = 2_000;

/// How long an entry's name may be, in characters.
///
/// The same bound `routes::context::MAX_TITLE_LENGTH` already puts on the title
/// of an ordinary knowledge entry, which is the only other path that writes one
/// into this column. Stated rather than derived from it: `db` does not import
/// `routes`.
pub const MAX_NAME_CHARS: usize = 256;

/// How many facts an index lists. A query `LIMIT`, so `i64`.
///
/// The same bound the two promoted-entry renderers already use, for the same
/// reason: past this many, an index costs more prompt than the recall it buys.
pub const MAX_FACTS: i64 = 40;

/// One kind of remembered entry.
///
/// Each variant is its own `knowledge_entries` category, so a kind can be
/// listed, rendered and forgotten without touching the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryCategory {
    /// Who the user is, in their own words. One entry.
    Profile,
    /// How the user wants the assistant to work. One entry.
    Preference,
    /// Anything else the user asked to have remembered. Many, named.
    Fact,
}

impl MemoryCategory {
    pub const ALL: [MemoryCategory; 3] = [Self::Profile, Self::Preference, Self::Fact];

    /// The stored `category` value. Carries [`MEMORY_CATEGORY_PREFIX`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => PROFILE_CATEGORY,
            Self::Preference => PREFERENCE_CATEGORY,
            Self::Fact => FACT_CATEGORY,
        }
    }

    /// The word the model uses, in every schema and every message.
    pub fn short(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Preference => "preference",
            Self::Fact => "fact",
        }
    }

    /// Accepts the model-facing word only.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.short() == text)
    }

    /// Total inverse of [`Self::as_str`], for a row read back.
    pub fn from_stored(stored: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.as_str() == stored)
    }

    pub fn heading(self) -> &'static str {
        match self {
            Self::Profile => "About the user",
            Self::Preference => "How the user wants you to work",
            Self::Fact => "Remembered for the user",
        }
    }

    pub fn preamble(self) -> &'static str {
        match self {
            Self::Profile => {
                "The user asked for this to be remembered about them. It is background and \
                 not a request: bring it up only where it changes the substance of the answer."
            }
            Self::Preference => {
                "The user asked you to work this way. Follow it unless this request \
                 contradicts it, and say so when you depart from one."
            }
            Self::Fact => {
                "The user asked for these to be remembered. Read one by name when it bears \
                 on the request; never raise one unprompted."
            }
        }
    }

    /// True for Profile and Preference, which hold one entry each.
    pub fn single(self) -> bool {
        self.title().is_some()
    }

    /// The forced title for a single-entry category.
    pub fn title(self) -> Option<&'static str> {
        match self {
            Self::Profile => Some(PROFILE_TITLE),
            Self::Preference => Some(PREFERENCES_TITLE),
            Self::Fact => None,
        }
    }
}

/// Renders the short form, so a frozen message builder cannot leak the stored
/// spelling into something the model reads.
impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.short())
    }
}

/// One remembered entry, read in full.
///
/// Not `Serialize`: nothing puts a memory row on the wire.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub content: String,
    pub category: String,
    pub version: i64,
    pub updated_at: Option<NaiveDateTime>,
}

/// What an index lists: enough to decide whether to read the entry, and the
/// version to quote when changing it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemoryIndexRow {
    pub title: String,
    pub description: Option<String>,
    pub category: String,
    pub version: i64,
}

/// One write, whole. `version` absent means create; present means replace what
/// was read at that version.
#[derive(Debug, Clone)]
pub struct MemoryWrite<'a> {
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub category: MemoryCategory,
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub content: &'a str,
    pub version: Option<i64>,
}

/// What a write did, so a caller reports it without re-reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryOutcome {
    Created {
        version: i64,
    },
    Updated {
        version: i64,
    },
    /// The row moved under the writer. Carries what it says now, to merge.
    Conflict {
        version: i64,
        content: String,
    },
    Missing,
    /// The composed content, or the description, would exceed [`MAX_ENTRY_CHARS`].
    /// Nothing was written.
    TooLong {
        limit: usize,
    },
    /// The name would not fit [`MAX_NAME_CHARS`]. Nothing was written.
    ///
    /// Its own outcome because its own remedy: shortening what the entry says
    /// never makes a name fit, and the name is checked first, so a writer told
    /// to be briefer would retry forever.
    NameTooLong {
        limit: usize,
    },
}

pub fn remembered(category: MemoryCategory, name: &str, version: i64) -> String {
    format!("Remembered: {category}/{name}, version {version}.")
}

pub fn updated(category: MemoryCategory, name: &str, version: i64) -> String {
    format!("Updated: {category}/{name}, now version {version}.")
}

pub fn appended(category: MemoryCategory, name: &str, version: i64) -> String {
    format!("Added to {category}/{name}, now version {version}.")
}

pub fn forgotten(category: MemoryCategory, name: &str) -> String {
    format!("Forgotten: {category}/{name}.")
}

/// Hands back what the entry says now, so the model merges without a second
/// round trip.
pub fn conflict(category: MemoryCategory, name: &str, version: i64, content: &str) -> String {
    format!(
        "{category}/{name} has changed since you read it, and is now version {version}. \
         It says:\n\n{content}\n\nMerge what you wanted to change into that and write again \
         with version {version}."
    )
}

pub fn missing(category: MemoryCategory, name: &str) -> String {
    format!("Nothing is remembered at {category}/{name}. memory_list shows what is.")
}

/// Over-length content is refused, never truncated: a silently shortened
/// memory is worse than one that was not written.
pub fn too_long(limit: usize) -> String {
    format!(
        "That is longer than one entry holds ({limit} characters). Say it more briefly, or \
         store it as a document instead."
    )
}

/// What [`too_long`] cannot say: the name is the half that has to change, and
/// the entry it names is allowed to be much longer than it is.
pub fn name_too_long(limit: usize) -> String {
    format!(
        "That name is longer than a name holds ({limit} characters). Give it a shorter name; \
         the entry itself may be longer."
    )
}

/// One statement, not two branches, so an unfiltered index and a filtered one
/// cannot drift apart. The `'memory-%'` literal is the same predicate the
/// workspace-wide reads carry negated, and a test pins it against
/// [`MEMORY_CATEGORY_PREFIX`].
///
/// The many-entry kind is bound rather than written in, and sorts last: a
/// person with more facts than the bound has one profile and one set of
/// preferences, and those two rows must not be the first the bound drops.
pub(crate) const INDEX_STATEMENT: &str = r#"
    SELECT title, description, category, version
    FROM knowledge_entries
    WHERE workspace_id = $1 AND created_by = $2 AND is_active = TRUE
      AND category LIKE 'memory-%'
      AND ($3::text IS NULL OR category = $3)
    ORDER BY category = $4, category, title
    LIMIT $5
"#;

pub(crate) const READ_STATEMENT: &str = r#"
    SELECT id, workspace_id, title, description, content, category, version, updated_at
    FROM knowledge_entries
    WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4
      AND is_active = TRUE
"#;

/// Zero rows is the conflict: the row moved, or was never this writer's.
pub(crate) const UPDATE_STATEMENT: &str = r#"
    UPDATE knowledge_entries
    SET content = $5, description = COALESCE($6, description), token_count = $7,
        version = version + 1, updated_at = NOW()
    WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4
      AND is_active = TRUE AND version = $8
    RETURNING version
"#;

/// Zero rows means it already exists. The `NOT EXISTS` is a check and not a
/// constraint, so it answers for the ordinary case and migration 034's partial
/// unique index answers for the raced one: the second of two creates that
/// overlap exactly raises a unique violation, which [`write`] reads as the same
/// conflict a zero-row result is.
pub(crate) const CREATE_STATEMENT: &str = r#"
    INSERT INTO knowledge_entries
      (workspace_id, created_by, category, title, description, content, token_count, version)
    SELECT $1, $2, $3, $4, $5, $6, $7, 1
    WHERE NOT EXISTS (
      SELECT 1 FROM knowledge_entries
      WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4
        AND is_active = TRUE)
    RETURNING version
"#;

/// Soft, and the token proves the row was read.
pub(crate) const FORGET_STATEMENT: &str = r#"
    UPDATE knowledge_entries
    SET is_active = FALSE, updated_at = NOW()
    WHERE workspace_id = $1 AND created_by = $2 AND category = $3 AND title = $4
      AND is_active = TRUE AND version = $5
"#;

/// A single-entry category has one title, so a name the model supplied for one
/// is ignored rather than read as a second entry it cannot reach again.
fn entry_title(category: MemoryCategory, supplied: &str) -> &str {
    category.title().unwrap_or(supplied)
}

fn token_count(content: &str) -> i32 {
    content.chars().count().div_ceil(CHARACTERS_PER_TOKEN) as i32
}

/// The limit `text` broke, if it broke one. Every half of an entry is bounded,
/// because every half is read back into a prompt on every turn.
fn overlong(text: &str, limit: usize) -> Option<usize> {
    (text.chars().count() > limit).then_some(limit)
}

/// A create that lost a race to an identical one, which migration 034's index
/// refuses. It means what a zero-row create means, so it is answered the same
/// way rather than handed to the caller as an error it can do nothing with.
fn raced(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

/// What a zero-row write means, answered by looking.
async fn moved_or_gone(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    category: MemoryCategory,
    title: &str,
) -> DbResult<MemoryOutcome> {
    match read(pool, workspace_id, user_id, category, title).await? {
        Some(row) => Ok(MemoryOutcome::Conflict {
            version: row.version,
            content: row.content,
        }),
        None => Ok(MemoryOutcome::Missing),
    }
}

/// Enough of one person's entries to decide which to read.
///
/// One row past [`MAX_FACTS`], which is what a caller shows: the extra row is
/// how it tells that the bound left something out rather than that the person
/// happens to have exactly that many.
pub async fn index(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    category: Option<MemoryCategory>,
) -> DbResult<Vec<MemoryIndexRow>> {
    sqlx::query_as::<_, MemoryIndexRow>(INDEX_STATEMENT)
        .bind(workspace_id)
        .bind(user_id)
        .bind(category.map(MemoryCategory::as_str))
        .bind(FACT_CATEGORY)
        .bind(MAX_FACTS + 1)
        .fetch_all(pool)
        .await
}

pub async fn read(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    category: MemoryCategory,
    title: &str,
) -> DbResult<Option<MemoryRow>> {
    sqlx::query_as::<_, MemoryRow>(READ_STATEMENT)
        .bind(workspace_id)
        .bind(user_id)
        .bind(category.as_str())
        .bind(entry_title(category, title))
        .fetch_optional(pool)
        .await
}

/// Create when no version is quoted, replace what was read at that version when
/// one is. Over-length content is refused whole: a memory silently shortened is
/// worse than one that was not written.
pub async fn write(pool: &PgPool, write: MemoryWrite<'_>) -> DbResult<MemoryOutcome> {
    let title = entry_title(write.category, write.title);
    if let Some(limit) = overlong(title, MAX_NAME_CHARS) {
        return Ok(MemoryOutcome::NameTooLong { limit });
    }
    let broken = write
        .description
        .and_then(|description| overlong(description, MAX_ENTRY_CHARS))
        .or_else(|| overlong(write.content, MAX_ENTRY_CHARS));
    if let Some(limit) = broken {
        return Ok(MemoryOutcome::TooLong { limit });
    }

    let written: Option<i64> = match write.version {
        Some(version) => {
            sqlx::query_scalar(UPDATE_STATEMENT)
                .bind(write.workspace_id)
                .bind(write.user_id)
                .bind(write.category.as_str())
                .bind(title)
                .bind(write.content)
                .bind(write.description)
                .bind(token_count(write.content))
                .bind(version)
                .fetch_optional(pool)
                .await?
        }
        None => {
            let created = sqlx::query_scalar(CREATE_STATEMENT)
                .bind(write.workspace_id)
                .bind(write.user_id)
                .bind(write.category.as_str())
                .bind(title)
                .bind(write.description)
                .bind(write.content)
                .bind(token_count(write.content))
                .fetch_optional(pool)
                .await;
            match created {
                Ok(version) => version,
                Err(error) if raced(&error) => None,
                Err(error) => return Err(error),
            }
        }
    };

    match (written, write.version) {
        (Some(version), Some(_)) => Ok(MemoryOutcome::Updated { version }),
        (Some(version), None) => Ok(MemoryOutcome::Created { version }),
        (None, _) => {
            moved_or_gone(
                pool,
                write.workspace_id,
                write.user_id,
                write.category,
                title,
            )
            .await
        }
    }
}

/// Add a line to an entry without asking the model to quote a version: the read
/// this does supplies one.
pub async fn append(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    category: MemoryCategory,
    title: &str,
    addition: &str,
) -> DbResult<MemoryOutcome> {
    let title = entry_title(category, title);
    if let Some(limit) = overlong(title, MAX_NAME_CHARS) {
        return Ok(MemoryOutcome::NameTooLong { limit });
    }

    let Some(existing) = read(pool, workspace_id, user_id, category, title).await? else {
        return Ok(MemoryOutcome::Missing);
    };

    let composed = format!("{}\n{addition}", existing.content);
    if composed.chars().count() > MAX_ENTRY_CHARS {
        return Ok(MemoryOutcome::TooLong {
            limit: MAX_ENTRY_CHARS,
        });
    }

    let written: Option<i64> = sqlx::query_scalar(UPDATE_STATEMENT)
        .bind(workspace_id)
        .bind(user_id)
        .bind(category.as_str())
        .bind(title)
        .bind(&composed)
        .bind(None::<&str>)
        .bind(token_count(&composed))
        .bind(existing.version)
        .fetch_optional(pool)
        .await?;

    match written {
        Some(version) => Ok(MemoryOutcome::Updated { version }),
        None => moved_or_gone(pool, workspace_id, user_id, category, title).await,
    }
}

/// Deactivate the entry the caller proved it had read. The returned version is
/// the one that was proved, not a new one: the row is gone from every read, so
/// there is no later version to quote.
pub async fn forget(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    category: MemoryCategory,
    title: &str,
    version: i64,
) -> DbResult<MemoryOutcome> {
    let title = entry_title(category, title);
    let forgotten = sqlx::query(FORGET_STATEMENT)
        .bind(workspace_id)
        .bind(user_id)
        .bind(category.as_str())
        .bind(title)
        .bind(version)
        .execute(pool)
        .await?
        .rows_affected();

    if forgotten > 0 {
        return Ok(MemoryOutcome::Updated { version });
    }

    moved_or_gone(pool, workspace_id, user_id, category, title).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::knowledge::{
        LearnedCategory, STANDING_INSTRUCTION_CATEGORY, reserved_namespace,
    };

    /// The three categories that were reserved before memory existed.
    fn existing_reserved() -> Vec<&'static str> {
        let mut categories = vec![STANDING_INSTRUCTION_CATEGORY];
        categories.extend(
            LearnedCategory::ALL
                .into_iter()
                .map(LearnedCategory::as_str),
        );
        categories
    }

    #[test]
    fn every_category_carries_the_private_prefix() {
        for category in MemoryCategory::ALL {
            assert!(
                category.as_str().starts_with(MEMORY_CATEGORY_PREFIX),
                "{} must start with {MEMORY_CATEGORY_PREFIX} or the read-path predicate cannot see it",
                category.as_str()
            );
        }
    }

    #[test]
    fn stored_names_round_trip() {
        for category in MemoryCategory::ALL {
            assert_eq!(
                MemoryCategory::from_stored(category.as_str()),
                Some(category)
            );
        }
    }

    #[test]
    fn short_names_round_trip() {
        for category in MemoryCategory::ALL {
            assert_eq!(MemoryCategory::parse(category.short()), Some(category));
        }
    }

    #[test]
    fn parse_rejects_the_stored_form() {
        for category in MemoryCategory::ALL {
            assert_eq!(
                MemoryCategory::parse(category.as_str()),
                None,
                "the stored spelling is not the model's word"
            );
        }
    }

    #[test]
    fn parse_rejects_every_category_the_learning_loop_owns() {
        for category in existing_reserved() {
            assert_eq!(MemoryCategory::parse(category), None, "{category}");
        }
    }

    #[test]
    fn from_stored_rejects_the_short_form() {
        for category in MemoryCategory::ALL {
            assert_eq!(
                MemoryCategory::from_stored(category.short()),
                None,
                "a row is stored under the prefixed spelling"
            );
        }
    }

    #[test]
    fn display_is_the_short_form() {
        for category in MemoryCategory::ALL {
            assert_eq!(category.to_string(), category.short());
        }
    }

    #[test]
    fn only_profile_and_preference_hold_one_entry() {
        assert!(MemoryCategory::Profile.single());
        assert!(MemoryCategory::Preference.single());
        assert!(!MemoryCategory::Fact.single());
        assert_eq!(MemoryCategory::Profile.title(), Some(PROFILE_TITLE));
        assert_eq!(MemoryCategory::Preference.title(), Some(PREFERENCES_TITLE));
        assert_eq!(MemoryCategory::Fact.title(), None);
        for category in MemoryCategory::ALL {
            assert_eq!(
                category.single(),
                category.title().is_some(),
                "{category} disagrees with itself about holding one entry"
            );
        }
    }

    #[test]
    fn headings_and_preambles_are_distinct_and_present() {
        for category in MemoryCategory::ALL {
            assert!(!category.heading().is_empty(), "{category}");
            assert!(!category.preamble().is_empty(), "{category}");
        }
        assert_eq!(MemoryCategory::Profile.heading(), "About the user");
        assert_eq!(
            MemoryCategory::Preference.heading(),
            "How the user wants you to work"
        );
        assert_eq!(MemoryCategory::Fact.heading(), "Remembered for the user");
        assert_eq!(
            MemoryCategory::Profile.preamble(),
            "The user asked for this to be remembered about them. It is background and not a \
             request: bring it up only where it changes the substance of the answer."
        );
        assert_eq!(
            MemoryCategory::Preference.preamble(),
            "The user asked you to work this way. Follow it unless this request contradicts \
             it, and say so when you depart from one."
        );
        assert_eq!(
            MemoryCategory::Fact.preamble(),
            "The user asked for these to be remembered. Read one by name when it bears on \
             the request; never raise one unprompted."
        );
    }

    #[test]
    fn remembered_names_the_entry_and_its_version() {
        assert_eq!(
            remembered(MemoryCategory::Fact, "Deploy window", 1),
            "Remembered: fact/Deploy window, version 1."
        );
    }

    #[test]
    fn updated_says_the_version_is_new() {
        assert_eq!(
            updated(MemoryCategory::Profile, PROFILE_TITLE, 4),
            "Updated: profile/Profile, now version 4."
        );
    }

    #[test]
    fn appended_says_the_version_is_new() {
        assert_eq!(
            appended(MemoryCategory::Preference, PREFERENCES_TITLE, 7),
            "Added to preference/Preferences, now version 7."
        );
    }

    #[test]
    fn forgotten_names_the_entry_and_no_version() {
        assert_eq!(
            forgotten(MemoryCategory::Fact, "Deploy window"),
            "Forgotten: fact/Deploy window."
        );
    }

    #[test]
    fn conflict_hands_back_the_current_content_and_the_version_to_retry_with() {
        assert_eq!(
            conflict(
                MemoryCategory::Fact,
                "Deploy window",
                3,
                "Thursdays, after standup."
            ),
            "fact/Deploy window has changed since you read it, and is now version 3. It \
             says:\n\nThursdays, after standup.\n\nMerge what you wanted to change into that \
             and write again with version 3."
        );
    }

    #[test]
    fn missing_points_at_the_index() {
        assert_eq!(
            missing(MemoryCategory::Fact, "Deploy window"),
            "Nothing is remembered at fact/Deploy window. memory_list shows what is."
        );
    }

    #[test]
    fn too_long_states_the_limit_and_offers_a_document() {
        assert_eq!(
            too_long(MAX_ENTRY_CHARS),
            "That is longer than one entry holds (2000 characters). Say it more briefly, or \
             store it as a document instead."
        );
    }

    #[test]
    fn name_too_long_asks_for_a_shorter_name_and_not_a_shorter_entry() {
        assert_eq!(
            name_too_long(MAX_NAME_CHARS),
            "That name is longer than a name holds (256 characters). Give it a shorter name; \
             the entry itself may be longer."
        );
        assert_ne!(
            name_too_long(MAX_NAME_CHARS),
            too_long(MAX_NAME_CHARS),
            "the entry limit worded for a name states a number that is not the entry limit and \
             asks for the change that cannot help"
        );
    }

    #[test]
    fn no_message_leaks_the_stored_spelling() {
        for category in MemoryCategory::ALL {
            let name = category.title().unwrap_or("Deploy window");
            let messages = [
                remembered(category, name, 1),
                updated(category, name, 2),
                appended(category, name, 3),
                forgotten(category, name),
                conflict(category, name, 4, "whatever it says"),
                missing(category, name),
            ];
            for message in messages {
                assert!(
                    !message.contains(MEMORY_CATEGORY_PREFIX),
                    "{message} leaks the stored spelling"
                );
                assert!(
                    message.contains(category.short()),
                    "{message} does not name the category the model asked for"
                );
            }
        }
    }

    #[test]
    fn the_knowledge_route_refuses_every_memory_category() {
        for category in MemoryCategory::ALL {
            assert!(
                reserved_namespace(Some(category.as_str()), &[]).is_some(),
                "{} must not be writable through the knowledge route",
                category.as_str()
            );
        }
    }

    #[test]
    fn memory_categories_do_not_collide_with_the_learning_loop() {
        for category in MemoryCategory::ALL {
            for reserved in existing_reserved() {
                assert_ne!(category.as_str(), reserved);
            }
        }
    }

    /// Every statement the store issues. Nothing else here reaches the table.
    fn statements() -> [(&'static str, &'static str); 5] {
        [
            ("INDEX_STATEMENT", INDEX_STATEMENT),
            ("READ_STATEMENT", READ_STATEMENT),
            ("UPDATE_STATEMENT", UPDATE_STATEMENT),
            ("CREATE_STATEMENT", CREATE_STATEMENT),
            ("FORGET_STATEMENT", FORGET_STATEMENT),
        ]
    }

    #[test]
    fn every_statement_scopes_by_workspace_owner_and_category() {
        for (name, statement) in statements() {
            assert!(
                statement.contains("workspace_id = $1"),
                "{name} does not scope by workspace"
            );
            assert!(
                statement.contains("created_by = $2"),
                "{name} does not scope by owner, so it reaches a row that is nobody's"
            );
            assert!(
                statement.contains("category = $3") || statement.contains("category LIKE"),
                "{name} does not scope by category"
            );
            assert!(
                statement.contains("is_active = TRUE"),
                "{name} reaches a forgotten row"
            );
        }
    }

    #[test]
    fn the_index_carries_the_private_prefix_it_shares_with_the_read_path() {
        assert!(
            INDEX_STATEMENT.contains(&format!("category LIKE '{MEMORY_CATEGORY_PREFIX}%'")),
            "the index must match on the same prefix the workspace-wide reads exclude"
        );
    }

    #[test]
    fn the_index_is_one_statement_with_an_optional_filter_and_a_bound() {
        assert_eq!(
            INDEX_STATEMENT.matches("SELECT").count(),
            1,
            "an unfiltered index and a filtered one must not be two statements"
        );
        assert!(INDEX_STATEMENT.contains("($3::text IS NULL OR category = $3)"));
        assert!(INDEX_STATEMENT.contains("LIMIT $5"));
    }

    #[test]
    fn the_index_sorts_the_one_entry_kinds_ahead_of_the_bound() {
        assert!(
            INDEX_STATEMENT.contains("ORDER BY category = $4, category, title"),
            "the kind that sorts last has to be the one the person has many of"
        );
        for category in MemoryCategory::ALL {
            assert!(
                !INDEX_STATEMENT.contains(category.as_str()),
                "{} is bound, not written into the statement",
                category.as_str()
            );
        }
    }

    #[test]
    fn an_update_matches_one_version_and_bumps_it() {
        assert!(UPDATE_STATEMENT.contains("AND version = $8"));
        assert!(UPDATE_STATEMENT.contains("version = version + 1"));
        assert!(
            UPDATE_STATEMENT.contains("RETURNING version"),
            "a zero-row result is the conflict, so the new version has to come back"
        );
    }

    #[test]
    fn a_create_refuses_to_overwrite_and_opens_at_version_one() {
        assert!(CREATE_STATEMENT.contains("WHERE NOT EXISTS"));
        assert!(CREATE_STATEMENT.contains("$7, 1"));
        assert!(CREATE_STATEMENT.contains("RETURNING version"));
    }

    #[test]
    fn forgetting_is_soft_and_proves_the_row_was_read() {
        assert!(FORGET_STATEMENT.contains("is_active = FALSE"));
        assert!(FORGET_STATEMENT.contains("AND version = $5"));
        assert!(
            !FORGET_STATEMENT.contains("DELETE"),
            "a forgotten entry is deactivated, never removed"
        );
    }

    #[test]
    fn a_single_entry_category_ignores_a_supplied_name() {
        assert_eq!(
            entry_title(MemoryCategory::Profile, "whatever the model called it"),
            PROFILE_TITLE
        );
        assert_eq!(
            entry_title(MemoryCategory::Preference, "whatever the model called it"),
            PREFERENCES_TITLE
        );
        assert_eq!(
            entry_title(MemoryCategory::Fact, "Deploy window"),
            "Deploy window"
        );
    }

    #[test]
    fn a_short_entry_never_counts_as_no_tokens() {
        assert_eq!(token_count(""), 0);
        assert_eq!(token_count("a"), 1);
        assert_eq!(token_count("abcd"), 1);
        assert_eq!(token_count("abcde"), 2);
    }

    #[test]
    fn a_token_count_measures_characters_and_not_bytes() {
        assert_eq!(token_count("字字字字"), 1);
    }
}

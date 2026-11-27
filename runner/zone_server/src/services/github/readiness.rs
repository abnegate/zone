//! Pull request readiness: a verdict that only turns green when the evidence
//! behind it is internally consistent.
//!
//! A provider can report `success` while the check list it returned does not
//! add up, and a paginated fetch can stop early and still look complete. Both
//! read as a pass to anything that only inspects the reported state. This
//! module refuses that: counts that do not close collapse to unknown, a
//! partially fetched thread or comment list is a named blocker on its own, and
//! a review only counts when it names the current head commit.
//!
//! Mergeability is held to the same standard. A conflict, a head behind its
//! base and a merge a branch protection rule blocks each read as their own
//! blocker, and a state GitHub has not worked out yet is unknown rather than
//! clean, because GitHub refuses the merge until it has.
//!
//! Which review bots are recognised is configuration, not a constant, and the
//! set is open: a bot this build has never heard of is read as a bot review
//! the moment GitHub says a bot wrote it. Vendor knowledge only makes a bot
//! easier to read, never a precondition for counting. Every bot states its own
//! scale, and a score is only ever compared against the bar of the scale it
//! was published on, so a perfect mark from one bot can never stand in for a
//! perfect mark from another.

use serde::Serialize;
use std::fmt;
use std::sync::LazyLock;

use super::link::absolute;

const SHA_LENGTH: usize = 40;
const SHORT_SHA_LENGTH: usize = 7;
const BOT_SUFFIX: &str = "[bot]";
const GREPTILE_REVIEWER: &str = "greptile-apps";
const GREPTILE_SCALE: u8 = 5;
const CODERABBIT_REVIEWER: &str = "coderabbitai";
const CODERABBIT_SCALE: u8 = 1;

/// A 40 character hexadecimal Git object name, lowercased.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        (value.len() == SHA_LENGTH && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| Self(value.to_ascii_lowercase()))
    }

    pub fn short(&self) -> &str {
        &self.0[..SHORT_SHA_LENGTH]
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The closed set of states a check summary may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Failure,
    None,
    Pending,
    Success,
    Unknown,
}

impl CheckState {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "failure" => Self::Failure,
            "none" => Self::None,
            "pending" => Self::Pending,
            "success" => Self::Success,
            "unknown" => Self::Unknown,
            _ => return Option::None,
        })
    }
}

/// Check tallies exactly as a provider reported them, before any validation.
///
/// The counts are signed so that an impossible report survives long enough to
/// be rejected rather than being made plausible by the type system.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckEvidence {
    pub state: String,
    pub complete: bool,
    pub total: i64,
    pub passed: i64,
    pub failed: i64,
    pub running: i64,
    pub queued: i64,
    pub in_progress: i64,
    pub unknown: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CheckCounts {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub queued: u32,
    pub in_progress: u32,
    pub unknown: u32,
}

/// Check evidence that has been proven to add up, or collapsed to unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CheckSummary {
    pub state: CheckState,
    pub complete: bool,
    pub counts: CheckCounts,
}

impl CheckEvidence {
    pub fn normalize(&self) -> CheckSummary {
        let state = CheckState::parse(&self.state);
        let counts = self.counts();
        let complete = self.complete
            && counts.is_some()
            && matches!(state, Some(state) if state != CheckState::Unknown);
        CheckSummary {
            state: if complete {
                state.unwrap_or(CheckState::Unknown)
            } else {
                CheckState::Unknown
            },
            complete,
            counts: counts.unwrap_or_default(),
        }
    }

    fn counts(&self) -> Option<CheckCounts> {
        let total = u32::try_from(self.total).ok()?;
        let passed = u32::try_from(self.passed).ok()?;
        let failed = u32::try_from(self.failed).ok()?;
        let running = u32::try_from(self.running).ok()?;
        let queued = u32::try_from(self.queued).ok()?;
        let in_progress = u32::try_from(self.in_progress).ok()?;
        let unknown = u32::try_from(self.unknown).ok()?;
        let reported = failed
            .checked_add(passed)?
            .checked_add(running)?
            .checked_add(unknown)?;
        (reported == total && in_progress.checked_add(queued)? == running).then_some(CheckCounts {
            total,
            passed,
            failed,
            running,
            queued,
            in_progress,
            unknown,
        })
    }
}

/// A review score and the scale it was expressed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Confidence {
    pub score: u8,
    pub scale: u8,
}

impl Confidence {
    pub fn satisfies(self, required: Self) -> bool {
        self.scale == required.scale && self.score >= required.score
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.score, self.scale)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}

/// The latest summary one review bot published on a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewSummary {
    pub reviewer: String,
    pub comment_id: u64,
    pub url: Option<String>,
    pub confidence: Option<Confidence>,
    pub reviewed_commit: Option<CommitSha>,
    pub created_at: String,
}

/// A review bot whose comments carry a confidence score and a reviewed commit.
///
/// An implementation owns every piece of vendor knowledge: the account it
/// publishes under, the bar it has to clear, the scale that bar is expressed
/// on, and how to read a score and a reviewed commit out of a comment body.
/// Nothing above this trait knows any of that.
pub trait ReviewSignal: Send + Sync {
    /// The account the bot reviews as, without the `[bot]` suffix.
    fn reviewer(&self) -> &str;

    fn required(&self) -> Confidence;

    fn identifies(&self, body: &str) -> bool;

    fn confidence(&self, body: &str) -> Option<Confidence>;

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha>;

    /// The bar the score it published has to clear.
    ///
    /// A bot whose scale this build knows is held to the bar it declared. A
    /// bot it does not know is held to full marks on whatever scale that bot
    /// used, because nothing here can tell which of that bot's scores means
    /// "ship it".
    fn requirement(&self, observed: Option<Confidence>) -> Confidence {
        let _ = observed;
        self.required()
    }

    /// Whether a comment author is this reviewer. GitHub reports a bot account
    /// as `name` over GraphQL and `name[bot]` over REST, so both are the
    /// reviewer.
    fn authored(&self, author: &str) -> bool {
        account(author) == account(self.reviewer())
    }

    fn summarize(&self, comments: &[ReviewComment]) -> Option<ReviewSummary> {
        comments
            .iter()
            .filter(|comment| self.authored(&comment.author) && self.identifies(&comment.body))
            .max_by_key(|comment| published_at(&comment.created_at))
            .map(|comment| ReviewSummary {
                reviewer: self.reviewer().to_string(),
                comment_id: comment.id,
                url: absolute(&comment.url),
                confidence: self.confidence(&comment.body),
                reviewed_commit: self.reviewed_commit(&comment.body),
                created_at: comment.created_at.clone(),
            })
    }
}

/// One review bot: either one this build has vendor knowledge of, or any
/// other bot account, read by the conventions those vendors share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalKind {
    CodeRabbit,
    Greptile,
    /// A bot named in configuration or met on a pull request, whose account is
    /// held here without the `[bot]` suffix.
    Other(String),
}

impl SignalKind {
    /// The bots this build has vendor knowledge of. Every other bot is read
    /// generically, so this is a head start, not the set that counts.
    pub fn known() -> [Self; 2] {
        [Self::CodeRabbit, Self::Greptile]
    }

    /// The bot a name refers to. Any account names a bot: one this build does
    /// not know is read generically rather than refused, so a repository can
    /// adopt a reviewer this build has never heard of. Only a blank name is
    /// no bot at all.
    pub fn parse(value: &str) -> Option<Self> {
        let account = account(value);
        Some(match account.as_str() {
            "" => return None,
            "coderabbit" | "coderabbitai" => Self::CodeRabbit,
            "greptile" | "greptile-apps" => Self::Greptile,
            _ => Self::Other(account),
        })
    }

    /// Whether this build knows the bot's format, rather than reading it by
    /// the conventions bots share.
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Other(_))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::CodeRabbit => "coderabbit",
            Self::Greptile => "greptile",
            Self::Other(account) => account,
        }
    }

    /// The account the bot comments as, without the `[bot]` suffix REST adds.
    pub fn reviewer(&self) -> &str {
        match self {
            Self::CodeRabbit => CODERABBIT_REVIEWER,
            Self::Greptile => GREPTILE_REVIEWER,
            Self::Other(account) => account,
        }
    }

    /// The comment that asks the bot to review, or review again, when it has
    /// not done so on its own.
    ///
    /// Only a bot whose command this build knows has one. A guess at an
    /// unknown bot's command would post a comment that bot ignores, so an
    /// unknown bot is waited for and never prodded.
    pub fn trigger_command(&self) -> Option<&'static str> {
        match self {
            Self::CodeRabbit => Some("@coderabbitai review"),
            Self::Greptile => Some("@greptileai review"),
            Self::Other(_) => None,
        }
    }

    /// Whether the bot can post its score in a pull request review instead of
    /// its summary comment. CodeRabbit on a paid plan does; Greptile never
    /// does; a bot this build does not know may, so its reviews are read.
    pub fn scores_in_reviews(&self) -> bool {
        match self {
            Self::CodeRabbit => true,
            Self::Greptile => false,
            Self::Other(_) => true,
        }
    }

    /// The one signal, for a caller that reads bots one at a time.
    pub fn as_signal(&self) -> Box<dyn ReviewSignal> {
        self.signal()
    }

    fn signal(&self) -> Box<dyn ReviewSignal> {
        match self {
            Self::CodeRabbit => Box::new(CodeRabbit),
            Self::Greptile => Box::new(Greptile),
            Self::Other(account) => Box::new(AnyBot::new(account)),
        }
    }
}

/// The set of review bots whose summaries count as review evidence.
///
/// The set is configuration. A deployment that names its bots recognises those
/// and every one of them that spoke has to clear its own bar. A deployment
/// that names none has an open set: whichever bot reviewed the pull request
/// counts, read by vendor knowledge where this build has it and generically
/// where it does not. A set explicitly emptied can never establish review
/// evidence, so it can never be ready.
pub struct ReviewSignals {
    signals: Vec<Box<dyn ReviewSignal>>,
    /// Whether a bot the set does not name also counts once it has reviewed.
    open: bool,
}

/// One recognised bot's latest summary, paired with the bar it must clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFinding {
    pub required: Confidence,
    pub summary: ReviewSummary,
}

impl ReviewSignals {
    /// The bots this build reads by default: the ones it knows, plus whichever
    /// other bot turns out to have reviewed the pull request.
    pub fn recognized() -> Self {
        Self {
            open: true,
            ..Self::of(SignalKind::known())
        }
    }

    pub fn empty() -> Self {
        Self {
            signals: Vec::new(),
            open: false,
        }
    }

    /// Exactly these bots, and no other.
    pub fn of(kinds: impl IntoIterator<Item = SignalKind>) -> Self {
        let mut chosen: Vec<SignalKind> = Vec::new();
        for kind in kinds {
            if !chosen.contains(&kind) {
                chosen.push(kind);
            }
        }
        Self::custom(chosen.iter().map(SignalKind::signal).collect())
    }

    pub fn custom(signals: Vec<Box<dyn ReviewSignal>>) -> Self {
        Self {
            signals,
            open: false,
        }
    }

    /// Build a set from configured names. Any account names a bot, so a bot
    /// this build has no vendor knowledge of is read generically rather than
    /// refused; only a blank name is rejected.
    pub fn select<Name: AsRef<str>>(names: &[Name]) -> Result<Self, String> {
        let mut kinds = Vec::with_capacity(names.len());
        for name in names {
            kinds.push(
                SignalKind::parse(name.as_ref())
                    .ok_or_else(|| "A review signal name cannot be blank.".to_string())?,
            );
        }
        Ok(Self::of(kinds))
    }

    pub fn reviewers(&self) -> Vec<String> {
        self.signals
            .iter()
            .map(|signal| signal.reviewer().to_string())
            .collect()
    }

    /// The bots a verdict waits for by name. An open set waits for no bot in
    /// particular: whichever one reviews satisfies it, so naming the bots this
    /// build happens to know would misreport what is being waited for.
    pub fn expected(&self) -> Vec<String> {
        if self.open {
            Vec::new()
        } else {
            self.reviewers()
        }
    }

    /// Whether a bot this set does not name counts once it has reviewed.
    pub fn open(&self) -> bool {
        self.open
    }

    pub fn len(&self) -> usize {
        self.signals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    /// The latest summary from every bot that published one: the bots this set
    /// names, and on an open set every other account GitHub marked a bot.
    ///
    /// A bot that said nothing produces no finding: silence is not evidence
    /// either way, and the missing-review blocker covers the case where every
    /// bot was silent.
    pub fn findings(&self, comments: &[ReviewComment]) -> Vec<ReviewFinding> {
        let unnamed = self.unnamed(comments);
        self.signals
            .iter()
            .chain(unnamed.iter())
            .filter_map(|signal| {
                signal.summarize(comments).map(|summary| ReviewFinding {
                    required: signal.requirement(summary.confidence),
                    summary,
                })
            })
            .collect()
    }

    /// A generic signal for every bot that commented and this set does not
    /// name, newest account order, once each.
    ///
    /// A bot is one GitHub marked as a bot, which it does by suffixing the
    /// login REST reports with `[bot]`. A person's comment is never read as a
    /// review summary, whatever it says.
    fn unnamed(&self, comments: &[ReviewComment]) -> Vec<Box<dyn ReviewSignal>> {
        if !self.open {
            return Vec::new();
        }
        let mut accounts: Vec<String> = Vec::new();
        for comment in comments {
            let Some(account) = bot_account(&comment.author) else {
                continue;
            };
            if accounts.contains(&account)
                || self
                    .signals
                    .iter()
                    .any(|signal| signal.authored(&comment.author))
            {
                continue;
            }
            accounts.push(account);
        }
        accounts
            .iter()
            .map(|account| Box::new(AnyBot::new(account)) as Box<dyn ReviewSignal>)
            .collect()
    }
}

impl Default for ReviewSignals {
    fn default() -> Self {
        Self::recognized()
    }
}

impl fmt::Debug for ReviewSignals {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ReviewSignals")
            .field(&self.reviewers())
            .finish()
    }
}

static CONFIDENCE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Confidence\s+Score\s*:\s*(\d{1,3})\s*/\s*(\d{1,3})")
        .expect("confidence score pattern")
});
static REVIEWED_LINE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Last\s+reviewed\s+commit\s*:?[^\r\n]*")
        .expect("reviewed commit line pattern")
});
static REVIEWED_SHA_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(?:^|[^0-9a-f])([0-9a-f]{40})(?:[^0-9a-f]|$)")
        .expect("reviewed commit pattern")
});
static MARKUP_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]*>|&nbsp;|[*_`]").expect("markup pattern"));
static ACTIONABLE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Actionable\s+comments\s+posted\s*:\s*(\d{1,4})")
        .expect("actionable comments pattern")
});
static REVIEWED_RANGE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)between\s+([0-9a-f]{40})\s+and\s+([0-9a-f]{40})")
        .expect("reviewed commit range pattern")
});

/// Greptile publishes one summary comment carrying `Confidence Score: n/5` and
/// `Last reviewed commit: <sha>`.
pub struct Greptile;

impl ReviewSignal for Greptile {
    /// The account this bot reviews as.
    fn reviewer(&self) -> &str {
        GREPTILE_REVIEWER
    }

    fn required(&self) -> Confidence {
        Confidence {
            score: GREPTILE_SCALE,
            scale: GREPTILE_SCALE,
        }
    }

    fn identifies(&self, body: &str) -> bool {
        let readable = readable(body);
        CONFIDENCE_PATTERN.is_match(&readable) || REVIEWED_LINE_PATTERN.is_match(&readable)
    }

    fn confidence(&self, body: &str) -> Option<Confidence> {
        stated_confidence(body)
    }

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha> {
        stated_commit(body)
    }
}

/// CodeRabbit publishes one walkthrough comment carrying
/// `Actionable comments posted: n` and a review range naming the base and the
/// head commit it read.
///
/// It scores nothing, so its scale is one: a review with no actionable comment
/// left on it is 1/1, and a review that left any is 0/1. That is a different
/// scale from a five-point score and never substitutes for one.
pub struct CodeRabbit;

impl ReviewSignal for CodeRabbit {
    /// The account this bot reviews as.
    fn reviewer(&self) -> &str {
        CODERABBIT_REVIEWER
    }

    fn required(&self) -> Confidence {
        Confidence {
            score: CODERABBIT_SCALE,
            scale: CODERABBIT_SCALE,
        }
    }

    fn identifies(&self, body: &str) -> bool {
        let readable = readable(body);
        ACTIONABLE_PATTERN.is_match(&readable) || REVIEWED_RANGE_PATTERN.is_match(&readable)
    }

    fn confidence(&self, body: &str) -> Option<Confidence> {
        actionable_confidence(body)
    }

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha> {
        ranged_commit(body)
    }
}

/// A bot this build has no vendor knowledge of, read by the conventions the
/// bots it does know share.
///
/// It takes what the bot's own summary states: a score, or the count of
/// actionable comments it posted, and the commit it says it reviewed. Nothing
/// is guessed at. A comment carrying neither mark is not a review summary and
/// is passed over; a summary whose score or reviewed commit cannot be read is
/// a named blocker rather than a pass, exactly as it is for a bot this build
/// knows.
pub struct AnyBot {
    account: String,
}

impl AnyBot {
    /// The bot commenting under `login`, with or without the `[bot]` suffix
    /// REST adds.
    pub fn new(login: &str) -> Self {
        Self {
            account: account(login),
        }
    }
}

impl ReviewSignal for AnyBot {
    fn reviewer(&self) -> &str {
        &self.account
    }

    /// The bar for a bot that stated no scale: a clean bill on the one-point
    /// scale a yes-or-no verdict is read on.
    fn required(&self) -> Confidence {
        Confidence {
            score: CODERABBIT_SCALE,
            scale: CODERABBIT_SCALE,
        }
    }

    /// Full marks on whatever scale this bot used. Which of an unknown bot's
    /// middling scores means "ship it" is the bot's own business, and reading
    /// one as approval would turn a reservation into a pass.
    fn requirement(&self, observed: Option<Confidence>) -> Confidence {
        observed.map_or_else(
            || self.required(),
            |observed| Confidence {
                score: observed.scale,
                scale: observed.scale,
            },
        )
    }

    fn identifies(&self, body: &str) -> bool {
        let readable = readable(body);
        CONFIDENCE_PATTERN.is_match(&readable)
            || ACTIONABLE_PATTERN.is_match(&readable)
            || REVIEWED_LINE_PATTERN.is_match(&readable)
            || REVIEWED_RANGE_PATTERN.is_match(&readable)
    }

    fn confidence(&self, body: &str) -> Option<Confidence> {
        stated_confidence(body).or_else(|| actionable_confidence(body))
    }

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha> {
        stated_commit(body).or_else(|| ranged_commit(body))
    }
}

/// The score a summary states as `Confidence Score: n/m`.
fn stated_confidence(body: &str) -> Option<Confidence> {
    let readable = readable(body);
    let captures = CONFIDENCE_PATTERN.captures(&readable)?;
    let score: u8 = captures.get(1)?.as_str().parse().ok()?;
    let scale: u8 = captures.get(2)?.as_str().parse().ok()?;
    (scale > 0 && score <= scale).then_some(Confidence { score, scale })
}

/// The score implied by `Actionable comments posted: n`, on the one-point
/// scale a count of outstanding comments is read on.
fn actionable_confidence(body: &str) -> Option<Confidence> {
    let readable = readable(body);
    let captures = ACTIONABLE_PATTERN.captures(&readable)?;
    let actionable: u32 = captures.get(1)?.as_str().parse().ok()?;
    Some(Confidence {
        score: u8::from(actionable == 0),
        scale: CODERABBIT_SCALE,
    })
}

/// The commit a `Last reviewed commit: <sha>` line names.
fn stated_commit(body: &str) -> Option<CommitSha> {
    let line = REVIEWED_LINE_PATTERN.find(body)?;
    let captures = REVIEWED_SHA_PATTERN.captures(line.as_str())?;
    CommitSha::parse(captures.get(1)?.as_str())
}

/// The head of a `between <base> and <head>` review range.
fn ranged_commit(body: &str) -> Option<CommitSha> {
    let readable = readable(body);
    let captures = REVIEWED_RANGE_PATTERN.captures(&readable)?;
    CommitSha::parse(captures.get(2)?.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub resolved: bool,
}

/// Review threads and whether every page of them was retrieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadEvidence {
    pub complete: bool,
    pub threads: Vec<ReviewThread>,
}

impl ThreadEvidence {
    pub fn unresolved(&self) -> u32 {
        self.threads
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }
}

/// Bot comments and whether every page of them was retrieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentEvidence {
    pub complete: bool,
    pub comments: Vec<ReviewComment>,
}

/// What GitHub says stands between a pull request and its base.
///
/// `Unknown` is the honest answer to two different silences: GitHub has not
/// worked the merge out yet, and GitHub named a state this does not know.
/// Neither establishes that the branch merges, and a merge asked for while the
/// answer is still unknown is refused, so neither can be read as mergeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mergeability {
    /// It merges into its base.
    Clean,
    /// A branch protection rule stands in the way.
    Blocked,
    /// The head is behind its base and has to be brought up to date first.
    Behind,
    /// The head conflicts with its base.
    Conflicted,
    /// It merges, but a check that is not required has failed.
    Unstable,
    /// It is a draft.
    Draft,
    /// GitHub has not worked it out, or named a state this does not know.
    #[default]
    Unknown,
}

impl Mergeability {
    /// Whether GitHub has established that the branch merges into its base.
    /// A failing check that nothing requires does not stand in the way of the
    /// merge; every other state either blocks it or was never established.
    pub fn merges(self) -> bool {
        matches!(self, Self::Clean | Self::Unstable)
    }
}

/// Everything observed about one pull request, before any judgement.
#[derive(Debug, Clone)]
pub struct PullEvidence {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub updated_at: String,
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub mergeable: Mergeability,
    pub threads: ThreadEvidence,
    pub comments: CommentEvidence,
    pub checks: CheckEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerCode {
    Draft,
    HeadCommitUnverified,
    MergeConflict,
    MergeBehind,
    MergeBlocked,
    MergeabilityUnverified,
    ThreadsIncomplete,
    CommentsIncomplete,
    UnresolvedThreads,
    ReviewMissing,
    ReviewLinkMissing,
    ConfidenceUnreadable,
    ConfidenceBelowRequirement,
    ReviewedCommitUnreadable,
    ReviewedCommitStale,
    ChecksFailed,
    ChecksPending,
    ChecksAbsent,
    ChecksIncomplete,
}

/// One reason a pull request is not ready, carrying the detail behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    Draft,
    HeadCommitUnverified,
    MergeConflict,
    MergeBehind,
    MergeBlocked,
    MergeabilityUnverified,
    ThreadsIncomplete,
    CommentsIncomplete,
    UnresolvedThreads(u32),
    ReviewMissing {
        /// The bots this deployment named, when it named any.
        reviewers: Vec<String>,
        /// Whether a review from a bot it never named would have counted.
        any_bot: bool,
    },
    ReviewLinkMissing {
        reviewer: String,
    },
    ConfidenceUnreadable {
        reviewer: String,
    },
    ConfidenceBelowRequirement {
        reviewer: String,
        observed: Confidence,
        required: Confidence,
    },
    ReviewedCommitUnreadable {
        reviewer: String,
    },
    ReviewedCommitStale {
        reviewer: String,
        reviewed: CommitSha,
        head: CommitSha,
    },
    ChecksFailed(CheckCounts),
    ChecksPending(CheckCounts),
    ChecksAbsent,
    ChecksIncomplete,
}

impl Blocker {
    pub fn code(&self) -> BlockerCode {
        match self {
            Self::Draft => BlockerCode::Draft,
            Self::HeadCommitUnverified => BlockerCode::HeadCommitUnverified,
            Self::MergeConflict => BlockerCode::MergeConflict,
            Self::MergeBehind => BlockerCode::MergeBehind,
            Self::MergeBlocked => BlockerCode::MergeBlocked,
            Self::MergeabilityUnverified => BlockerCode::MergeabilityUnverified,
            Self::ThreadsIncomplete => BlockerCode::ThreadsIncomplete,
            Self::CommentsIncomplete => BlockerCode::CommentsIncomplete,
            Self::UnresolvedThreads(_) => BlockerCode::UnresolvedThreads,
            Self::ReviewMissing { .. } => BlockerCode::ReviewMissing,
            Self::ReviewLinkMissing { .. } => BlockerCode::ReviewLinkMissing,
            Self::ConfidenceUnreadable { .. } => BlockerCode::ConfidenceUnreadable,
            Self::ConfidenceBelowRequirement { .. } => BlockerCode::ConfidenceBelowRequirement,
            Self::ReviewedCommitUnreadable { .. } => BlockerCode::ReviewedCommitUnreadable,
            Self::ReviewedCommitStale { .. } => BlockerCode::ReviewedCommitStale,
            Self::ChecksFailed(_) => BlockerCode::ChecksFailed,
            Self::ChecksPending(_) => BlockerCode::ChecksPending,
            Self::ChecksAbsent => BlockerCode::ChecksAbsent,
            Self::ChecksIncomplete => BlockerCode::ChecksIncomplete,
        }
    }

    pub fn report(&self) -> BlockerReport {
        BlockerReport {
            code: self.code(),
            detail: self.to_string(),
        }
    }
}

impl fmt::Display for Blocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => formatter.write_str("Pull request is still a draft"),
            Self::HeadCommitUnverified => {
                formatter.write_str("Head commit could not be established from the observations")
            }
            Self::MergeConflict => {
                formatter.write_str("Head conflicts with its base branch")
            }
            Self::MergeBehind => formatter.write_str(
                "Head is behind its base branch and has to be brought up to date first",
            ),
            Self::MergeBlocked => {
                formatter.write_str("GitHub reports the merge blocked by a branch protection rule")
            }
            Self::MergeabilityUnverified => formatter.write_str(
                "GitHub has not established whether the branch merges, and refuses a merge until it has",
            ),
            Self::ThreadsIncomplete => {
                formatter.write_str("Review threads could not be fully checked")
            }
            Self::CommentsIncomplete => {
                formatter.write_str("Review comments could not be fully checked")
            }
            Self::UnresolvedThreads(count) => write!(
                formatter,
                "{count} unresolved review {}",
                if *count == 1 { "thread" } else { "threads" }
            ),
            Self::ReviewMissing {
                reviewers,
                any_bot: true,
            } if reviewers.is_empty() => {
                formatter.write_str("No bot has reviewed this commit")
            }
            Self::ReviewMissing { reviewers, .. } if reviewers.is_empty() => formatter.write_str(
                "No review signal is recognised, so no review evidence can be established",
            ),
            Self::ReviewMissing { reviewers, .. } => {
                write!(formatter, "{} summary missing", reviewers.join(", "))
            }
            Self::ReviewLinkMissing { reviewer } => {
                write!(formatter, "{reviewer} review link missing")
            }
            Self::ConfidenceUnreadable { reviewer } => {
                write!(formatter, "{reviewer} confidence missing or unreadable")
            }
            Self::ConfidenceBelowRequirement {
                reviewer,
                observed,
                required,
            } => write!(
                formatter,
                "{reviewer} confidence {observed}; {required} required"
            ),
            Self::ReviewedCommitUnreadable { reviewer } => write!(
                formatter,
                "{reviewer} last reviewed commit missing or unreadable"
            ),
            Self::ReviewedCommitStale {
                reviewer,
                reviewed,
                head,
            } => write!(
                formatter,
                "{reviewer} reviewed {}; head is {}",
                reviewed.short(),
                head.short()
            ),
            Self::ChecksFailed(counts) => write!(
                formatter,
                "{} of {} checks failed",
                counts.failed, counts.total
            ),
            Self::ChecksPending(counts) => write!(
                formatter,
                "{} of {} checks still running",
                counts.running, counts.total
            ),
            Self::ChecksAbsent => formatter.write_str("No checks were observed for this commit"),
            Self::ChecksIncomplete => {
                formatter.write_str("Checks could not be fully checked or did not add up")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockerReport {
    pub code: BlockerCode,
    pub detail: String,
}

/// The verdict for one pull request and every reason it is not ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub updated_at: String,
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub mergeable: Mergeability,
    pub ready: bool,
    pub blockers: Vec<Blocker>,
    pub checks: CheckSummary,
    pub reviews: Vec<ReviewSummary>,
    pub review_current: bool,
    pub unresolved: u32,
    pub threads_complete: bool,
    pub comments_complete: bool,
}

impl Assessment {
    pub fn reports(&self) -> Vec<BlockerReport> {
        self.blockers.iter().map(Blocker::report).collect()
    }
}

/// Judge one pull request. `ready` is the conjunction of every positive
/// condition, so evidence that was never established can never satisfy it.
///
/// Nothing here knows which bots exist. Every recognised bot that published a
/// summary has to clear the bar it declared, on the scale it declared, and at
/// least one has to have spoken at all.
pub fn assess(evidence: PullEvidence, signals: &ReviewSignals) -> Assessment {
    // `draft` is the one state two observations report: the pull request's own
    // flag, and the mergeability GitHub sums up as `draft`. Either of them is
    // a draft, named once.
    let draft = evidence.draft || evidence.mergeable == Mergeability::Draft;
    let checks = evidence.checks.normalize();
    let findings = signals.findings(&evidence.comments.comments);
    let unresolved = evidence.threads.unresolved();
    let threads_complete = evidence.threads.complete;
    let comments_complete = evidence.comments.complete;
    let review_current = !findings.is_empty()
        && findings.iter().all(|finding| {
            matches!(
                (&finding.summary.reviewed_commit, &evidence.head),
                (Some(reviewed), Some(head)) if reviewed == head
            )
        });
    let reviews_satisfied = !findings.is_empty()
        && findings.iter().all(|finding| {
            finding.summary.url.is_some()
                && finding
                    .summary
                    .confidence
                    .is_some_and(|observed| observed.satisfies(finding.required))
        });
    let mut blockers = Vec::new();

    if draft {
        blockers.push(Blocker::Draft);
    }
    if evidence.head.is_none() {
        blockers.push(Blocker::HeadCommitUnverified);
    }
    // The draft blocker above has already named a draft, so it is not said
    // twice here. It still never reads as merging.
    match evidence.mergeable {
        Mergeability::Conflicted => blockers.push(Blocker::MergeConflict),
        Mergeability::Behind => blockers.push(Blocker::MergeBehind),
        Mergeability::Blocked => blockers.push(Blocker::MergeBlocked),
        Mergeability::Unknown => blockers.push(Blocker::MergeabilityUnverified),
        Mergeability::Clean | Mergeability::Unstable | Mergeability::Draft => {}
    }
    if !threads_complete {
        blockers.push(Blocker::ThreadsIncomplete);
    }
    if !comments_complete {
        blockers.push(Blocker::CommentsIncomplete);
    }
    if unresolved > 0 {
        blockers.push(Blocker::UnresolvedThreads(unresolved));
    }

    if findings.is_empty() {
        blockers.push(Blocker::ReviewMissing {
            reviewers: signals.expected(),
            any_bot: signals.open(),
        });
    }
    for finding in &findings {
        let reviewer = finding.summary.reviewer.clone();
        match finding.summary.confidence {
            None => blockers.push(Blocker::ConfidenceUnreadable {
                reviewer: reviewer.clone(),
            }),
            Some(observed) if !observed.satisfies(finding.required) => {
                blockers.push(Blocker::ConfidenceBelowRequirement {
                    reviewer: reviewer.clone(),
                    observed,
                    required: finding.required,
                });
            }
            Some(_) => {}
        }
        match (&finding.summary.reviewed_commit, &evidence.head) {
            (None, _) => blockers.push(Blocker::ReviewedCommitUnreadable {
                reviewer: reviewer.clone(),
            }),
            (Some(reviewed), Some(head)) if reviewed != head => {
                blockers.push(Blocker::ReviewedCommitStale {
                    reviewer: reviewer.clone(),
                    reviewed: reviewed.clone(),
                    head: head.clone(),
                });
            }
            _ => {}
        }
        if finding.summary.url.is_none() {
            blockers.push(Blocker::ReviewLinkMissing { reviewer });
        }
    }

    match checks.state {
        CheckState::Failure => blockers.push(Blocker::ChecksFailed(checks.counts)),
        CheckState::Pending => blockers.push(Blocker::ChecksPending(checks.counts)),
        CheckState::None => blockers.push(Blocker::ChecksAbsent),
        CheckState::Unknown => blockers.push(Blocker::ChecksIncomplete),
        CheckState::Success => {}
    }

    let ready = !draft
        && evidence.head.is_some()
        && evidence.mergeable.merges()
        && threads_complete
        && comments_complete
        && unresolved == 0
        && reviews_satisfied
        && review_current
        && checks.complete
        && checks.state == CheckState::Success;

    Assessment {
        number: evidence.number,
        title: evidence.title,
        url: evidence.url,
        updated_at: evidence.updated_at,
        draft,
        head: evidence.head,
        mergeable: evidence.mergeable,
        ready,
        blockers,
        checks,
        reviews: findings
            .into_iter()
            .map(|finding| finding.summary)
            .collect(),
        review_current,
        unresolved,
        threads_complete,
        comments_complete,
    }
}

fn readable(body: &str) -> String {
    MARKUP_PATTERN
        .replace_all(body, " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn account(login: &str) -> String {
    login
        .trim()
        .trim_end_matches(BOT_SUFFIX)
        .to_ascii_lowercase()
}

/// The account behind a login GitHub marked as a bot's.
///
/// GitHub suffixes a bot account's login with `[bot]` on the REST responses
/// every comment weighed here is read from. A login without that suffix
/// belongs to a person as far as this evidence can tell, and a person's
/// comment is never read as a bot review.
fn bot_account(login: &str) -> Option<String> {
    let account = account(login.trim().strip_suffix(BOT_SUFFIX)?);
    (!account.is_empty()).then_some(account)
}

pub(crate) fn published_at(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(i64::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OLDER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn green_checks() -> CheckEvidence {
        CheckEvidence {
            state: "success".into(),
            complete: true,
            total: 4,
            passed: 4,
            failed: 0,
            running: 0,
            queued: 0,
            in_progress: 0,
            unknown: 0,
        }
    }

    fn summary_body(score: u8, sha: &str) -> String {
        format!("## Greptile\n**Confidence Score:** {score}/5\n_Last reviewed commit: {sha}_\n")
    }

    fn comment(body: &str) -> ReviewComment {
        ReviewComment {
            id: 11,
            author: GREPTILE_REVIEWER.into(),
            body: body.to_string(),
            url: "https://github.com/owner/repository/pull/7#issuecomment-11".into(),
            created_at: "2026-09-05T00:00:00Z".into(),
        }
    }

    fn green_pull() -> PullEvidence {
        PullEvidence {
            number: 7,
            title: "Ship the billing export".into(),
            url: "https://github.com/owner/repository/pull/7".into(),
            updated_at: "2026-09-05T00:00:00Z".into(),
            draft: false,
            head: CommitSha::parse(HEAD),
            mergeable: Mergeability::Clean,
            threads: ThreadEvidence {
                complete: true,
                threads: vec![ReviewThread { resolved: true }],
            },
            comments: CommentEvidence {
                complete: true,
                comments: vec![comment(&summary_body(5, HEAD))],
            },
            checks: green_checks(),
        }
    }

    #[test]
    fn counts_that_do_not_sum_collapse_to_unknown() {
        let summary = CheckEvidence {
            total: 5,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts, CheckCounts::default());
    }

    #[test]
    fn split_running_counts_that_do_not_sum_collapse_to_unknown() {
        let summary = CheckEvidence {
            state: "pending".into(),
            complete: true,
            total: 4,
            passed: 2,
            failed: 0,
            running: 2,
            queued: 1,
            in_progress: 0,
            unknown: 0,
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts.running, 0);

        let coherent = CheckEvidence {
            state: "pending".into(),
            complete: true,
            total: 4,
            passed: 2,
            failed: 0,
            running: 2,
            queued: 1,
            in_progress: 1,
            unknown: 0,
        }
        .normalize();
        assert_eq!(coherent.state, CheckState::Pending);
        assert!(coherent.complete);
        assert_eq!(coherent.counts.running, 2);
    }

    #[test]
    fn negative_counts_collapse_to_unknown() {
        let summary = CheckEvidence {
            total: 3,
            passed: 5,
            failed: -2,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts.failed, 0);
    }

    #[test]
    fn unrecognized_and_declared_unknown_states_collapse() {
        for state in ["", "SUCCESS", "green", "passed", "ok", "unknown"] {
            let summary = CheckEvidence {
                state: state.into(),
                ..green_checks()
            }
            .normalize();
            assert_eq!(summary.state, CheckState::Unknown, "state {state}");
            assert!(!summary.complete, "state {state}");
        }
    }

    #[test]
    fn a_partial_fetch_is_never_complete_even_when_the_counts_close() {
        let summary = CheckEvidence {
            complete: false,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(
            summary.counts.passed, 4,
            "coherent counts stay readable even when the fetch was partial"
        );
    }

    #[test]
    fn every_closed_state_survives_coherent_counts() {
        for (state, expected) in [
            ("success", CheckState::Success),
            ("failure", CheckState::Failure),
            ("pending", CheckState::Pending),
            ("none", CheckState::None),
        ] {
            let summary = CheckEvidence {
                state: state.into(),
                ..green_checks()
            }
            .normalize();
            assert_eq!(summary.state, expected);
            assert!(summary.complete);
        }
    }

    #[test]
    fn incomplete_threads_block_an_otherwise_green_pull_request() {
        let assessment = assess(
            PullEvidence {
                threads: ThreadEvidence {
                    complete: false,
                    threads: vec![ReviewThread { resolved: true }],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports(),
            vec![BlockerReport {
                code: BlockerCode::ThreadsIncomplete,
                detail: "Review threads could not be fully checked".into(),
            }]
        );
    }

    #[test]
    fn incomplete_comments_block_an_otherwise_green_pull_request() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: false,
                    comments: vec![comment(&summary_body(5, HEAD))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.blockers,
            vec![Blocker::CommentsIncomplete],
            "a review found in a partial list is not proof there is nothing else"
        );
    }

    #[test]
    fn a_review_of_an_older_commit_blocks_and_names_both_commits() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(&summary_body(5, OLDER))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert!(!assessment.review_current);
        assert_eq!(
            assessment.reports()[0].code,
            BlockerCode::ReviewedCommitStale
        );
        assert_eq!(
            assessment.reports()[0].detail,
            "greptile-apps reviewed bbbbbbb; head is aaaaaaa"
        );
    }

    #[test]
    fn an_unverified_head_commit_blocks_even_with_a_full_score() {
        let assessment = assess(
            PullEvidence {
                head: CommitSha::parse("not-a-commit"),
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert!(assessment.blockers.contains(&Blocker::HeadCommitUnverified));
        assert!(!assessment.review_current);
    }

    #[test]
    fn a_short_score_blocks_and_reports_the_requirement() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(&summary_body(4, HEAD))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports()[0].detail,
            "greptile-apps confidence 4/5; 5/5 required"
        );
    }

    #[test]
    fn a_score_on_another_scale_never_satisfies_the_requirement() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(
                        "Confidence Score: 10/10\nLast reviewed commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports()[0].code,
            BlockerCode::ConfidenceBelowRequirement
        );
    }

    #[test]
    fn unresolved_threads_and_missing_reviews_are_named_separately() {
        let assessment = assess(
            PullEvidence {
                threads: ThreadEvidence {
                    complete: true,
                    threads: vec![
                        ReviewThread { resolved: false },
                        ReviewThread { resolved: false },
                        ReviewThread { resolved: true },
                    ],
                },
                comments: CommentEvidence {
                    complete: true,
                    comments: Vec::new(),
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.unresolved, 2);
        assert_eq!(
            assessment
                .reports()
                .into_iter()
                .map(|report| report.detail)
                .collect::<Vec<_>>(),
            vec![
                "2 unresolved review threads".to_string(),
                "No bot has reviewed this commit".to_string(),
            ],
            "a deployment that named no bot waits for whichever one reviews, so \
             the blocker names none of them"
        );
        assert_eq!(
            assess(
                PullEvidence {
                    comments: CommentEvidence {
                        complete: true,
                        comments: Vec::new(),
                    },
                    ..green_pull()
                },
                &ReviewSignals::select(&["appwrite"]).expect("a named bot"),
            )
            .reports()
            .into_iter()
            .map(|report| report.detail)
            .collect::<Vec<_>>(),
            vec!["appwrite summary missing".to_string()],
            "a deployment that named its bot is told which one is missing"
        );
    }

    #[test]
    fn a_review_without_a_link_blocks() {
        let mut unlinked = comment(&summary_body(5, HEAD));
        unlinked.url = "  ".into();
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![unlinked],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.reports()[0].code, BlockerCode::ReviewLinkMissing);
    }

    #[test]
    fn a_draft_is_never_ready() {
        let assessment = assess(
            PullEvidence {
                draft: true,
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.blockers, vec![Blocker::Draft]);
    }

    #[test]
    fn a_branch_github_does_not_report_as_merging_is_never_ready() {
        for (mergeable, code) in [
            (Mergeability::Conflicted, BlockerCode::MergeConflict),
            (Mergeability::Behind, BlockerCode::MergeBehind),
            (Mergeability::Blocked, BlockerCode::MergeBlocked),
            (Mergeability::Unknown, BlockerCode::MergeabilityUnverified),
        ] {
            let assessment = assess(
                PullEvidence {
                    mergeable,
                    ..green_pull()
                },
                &ReviewSignals::recognized(),
            );
            assert!(!assessment.ready, "{mergeable:?} read as ready");
            assert_eq!(
                assessment.reports()[0].code,
                code,
                "{mergeable:?} was not named"
            );
            assert_eq!(assessment.mergeable, mergeable);
        }
    }

    /// A failing check nothing requires is what `unstable` means, and the
    /// check evidence judges it on its own. The merge itself is not blocked,
    /// so mergeability does not block it a second time.
    #[test]
    fn an_unstable_branch_merges_and_leaves_the_judgement_to_the_checks() {
        let assessment = assess(
            PullEvidence {
                mergeable: Mergeability::Unstable,
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(assessment.ready, "{:?}", assessment.blockers);
        assert_eq!(assessment.mergeable, Mergeability::Unstable);

        let failing = assess(
            PullEvidence {
                mergeable: Mergeability::Unstable,
                checks: CheckEvidence {
                    state: "failure".into(),
                    failed: 1,
                    passed: 3,
                    ..green_checks()
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!failing.ready);
        assert_eq!(failing.reports()[0].code, BlockerCode::ChecksFailed);
    }

    #[test]
    fn a_draft_is_not_blocked_twice_for_being_one() {
        let assessment = assess(
            PullEvidence {
                draft: true,
                mergeable: Mergeability::Draft,
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.blockers, vec![Blocker::Draft]);
    }

    #[test]
    fn a_mergeability_nothing_observed_is_unknown_and_does_not_merge() {
        assert_eq!(Mergeability::default(), Mergeability::Unknown);
        assert!(!Mergeability::default().merges());
        for state in [
            Mergeability::Conflicted,
            Mergeability::Behind,
            Mergeability::Blocked,
            Mergeability::Draft,
        ] {
            assert!(!state.merges(), "{state:?}");
        }
    }

    #[test]
    fn every_failing_check_state_is_named() {
        for (state, code) in [
            ("failure", BlockerCode::ChecksFailed),
            ("pending", BlockerCode::ChecksPending),
            ("none", BlockerCode::ChecksAbsent),
            ("unknown", BlockerCode::ChecksIncomplete),
            ("nonsense", BlockerCode::ChecksIncomplete),
        ] {
            let assessment = assess(
                PullEvidence {
                    checks: CheckEvidence {
                        state: state.into(),
                        ..green_checks()
                    },
                    ..green_pull()
                },
                &ReviewSignals::recognized(),
            );
            assert!(!assessment.ready, "state {state}");
            assert_eq!(assessment.reports()[0].code, code, "state {state}");
        }
    }

    #[test]
    fn a_reported_success_over_counts_that_do_not_add_up_is_not_ready() {
        let assessment = assess(
            PullEvidence {
                checks: CheckEvidence {
                    state: "success".into(),
                    complete: true,
                    total: 9,
                    passed: 4,
                    ..green_checks()
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.checks.state, CheckState::Unknown);
        assert_eq!(assessment.blockers, vec![Blocker::ChecksIncomplete]);
    }

    #[test]
    fn a_fully_green_pull_request_is_ready_with_no_blockers() {
        let assessment = assess(green_pull(), &ReviewSignals::recognized());
        assert!(assessment.ready);
        assert!(assessment.blockers.is_empty());
        assert!(assessment.review_current);
        assert_eq!(assessment.checks.state, CheckState::Success);
        assert_eq!(assessment.checks.counts.passed, 4);
        assert_eq!(assessment.reviews.len(), 1);
        assert_eq!(
            assessment.reviews[0].confidence,
            Some(Confidence { score: 5, scale: 5 })
        );
        assert_eq!(
            assessment.reviews[0].reviewed_commit,
            CommitSha::parse(HEAD)
        );
    }

    #[test]
    fn readiness_is_exactly_the_absence_of_blockers_across_every_combination() {
        let states = ["success", "failure", "pending", "none", "unknown", "bogus"];
        let bodies = [
            summary_body(5, HEAD),
            summary_body(4, HEAD),
            summary_body(5, OLDER),
            "no summary here".to_string(),
            "Confidence Score: unreadable\nLast reviewed commit: none".to_string(),
        ];
        let mergeabilities = [
            Mergeability::Clean,
            Mergeability::Unstable,
            Mergeability::Blocked,
            Mergeability::Behind,
            Mergeability::Conflicted,
            Mergeability::Draft,
            Mergeability::Unknown,
        ];
        let mut ready_cases = 0;
        for draft in [false, true] {
            for head in [CommitSha::parse(HEAD), None] {
                for threads_complete in [false, true] {
                    for comments_complete in [false, true] {
                        for resolved in [false, true] {
                            for body in &bodies {
                                for state in states {
                                    for total in [4, 5] {
                                        for mergeable in mergeabilities {
                                            let assessment = assess(
                                                PullEvidence {
                                                    draft,
                                                    head: head.clone(),
                                                    mergeable,
                                                    threads: ThreadEvidence {
                                                        complete: threads_complete,
                                                        threads: vec![ReviewThread { resolved }],
                                                    },
                                                    comments: CommentEvidence {
                                                        complete: comments_complete,
                                                        comments: vec![comment(body)],
                                                    },
                                                    checks: CheckEvidence {
                                                        state: state.into(),
                                                        total,
                                                        ..green_checks()
                                                    },
                                                    ..green_pull()
                                                },
                                                &ReviewSignals::recognized(),
                                            );
                                            assert_eq!(
                                                assessment.ready,
                                                assessment.blockers.is_empty(),
                                                "ready and blockers disagree for draft={draft} head={head:?} mergeable={mergeable:?} threads={threads_complete} comments={comments_complete} resolved={resolved} state={state} total={total} body={body:?}"
                                            );
                                            ready_cases += usize::from(assessment.ready);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            ready_cases, 2,
            "only a clean and an unstable merge should describe a ready pull request"
        );
    }

    #[test]
    fn only_the_latest_recognized_comment_from_the_reviewer_is_used() {
        let comments = vec![
            ReviewComment {
                id: 1,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(5, OLDER),
                url: "https://example.test/1".into(),
                created_at: "2026-09-01T00:00:00Z".into(),
            },
            ReviewComment {
                id: 2,
                author: "someone-else".into(),
                body: summary_body(5, HEAD),
                url: "https://example.test/2".into(),
                created_at: "2026-09-09T00:00:00Z".into(),
            },
            ReviewComment {
                id: 3,
                author: GREPTILE_REVIEWER.into(),
                body: "Looks good to me".into(),
                url: "https://example.test/3".into(),
                created_at: "2026-09-08T00:00:00Z".into(),
            },
            ReviewComment {
                id: 4,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(3, HEAD),
                url: "https://example.test/4".into(),
                created_at: "2026-09-07T00:00:00Z".into(),
            },
        ];
        let summary = Greptile.summarize(&comments).unwrap();
        assert_eq!(summary.comment_id, 4);
        assert_eq!(summary.confidence, Some(Confidence { score: 3, scale: 5 }));
        assert_eq!(summary.reviewed_commit, CommitSha::parse(HEAD));
        assert_eq!(summary.reviewer, GREPTILE_REVIEWER);
    }

    #[test]
    fn an_unparsable_timestamp_never_outranks_a_readable_one() {
        let comments = vec![
            ReviewComment {
                id: 1,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(5, HEAD),
                url: "https://example.test/1".into(),
                created_at: "2026-09-01T00:00:00Z".into(),
            },
            ReviewComment {
                id: 2,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(1, OLDER),
                url: "https://example.test/2".into(),
                created_at: "yesterday".into(),
            },
        ];
        assert_eq!(Greptile.summarize(&comments).unwrap().comment_id, 1);
    }

    #[test]
    fn a_summary_links_only_to_an_absolute_web_address() {
        let linked = comment(&summary_body(5, HEAD));
        assert_eq!(
            Greptile
                .summarize(std::slice::from_ref(&linked))
                .unwrap()
                .url
                .as_deref(),
            Some(linked.url.as_str())
        );
        for url in [
            "/owner/repository/pull/7#issuecomment-11",
            "javascript:x",
            " ",
        ] {
            let unlinked = ReviewComment {
                url: url.into(),
                ..linked.clone()
            };
            assert_eq!(Greptile.summarize(&[unlinked]).unwrap().url, None, "{url}");
        }
    }

    #[test]
    fn greptile_parses_scores_and_commits_through_markup() {
        let body = "<p><strong>Confidence&nbsp;Score:</strong> 5&nbsp;/&nbsp;5</p>\n\
                    <p>_Last reviewed commit: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`_</p>";
        assert_eq!(
            Greptile.confidence(body),
            Some(Confidence { score: 5, scale: 5 })
        );
        assert_eq!(Greptile.reviewed_commit(body), CommitSha::parse(HEAD));
        assert!(Greptile.identifies(body));
    }

    #[test]
    fn greptile_rejects_scores_and_commits_it_cannot_read() {
        assert!(
            Greptile
                .confidence("Confidence Score: five out of five")
                .is_none()
        );
        assert!(Greptile.confidence("Confidence Score: 6/5").is_none());
        assert!(Greptile.confidence("Confidence Score: 3/0").is_none());
        assert!(
            Greptile
                .reviewed_commit("Last reviewed commit: unknown")
                .is_none()
        );
        assert!(
            Greptile
                .reviewed_commit(&format!("Last reviewed commit: {HEAD}a"))
                .is_none(),
            "a longer hex run is not a commit name"
        );
        assert!(
            Greptile
                .reviewed_commit(&format!("Merged {HEAD}\nLast reviewed commit: unknown"))
                .is_none(),
            "a commit outside the reviewed line must not be adopted"
        );
        assert!(!Greptile.identifies("Nice work!"));
    }

    #[test]
    fn commit_names_are_normalized_and_validated() {
        let upper = CommitSha::parse(&HEAD.to_ascii_uppercase()).unwrap();
        assert_eq!(upper, CommitSha::parse(HEAD).unwrap());
        assert_eq!(upper.short(), "aaaaaaa");
        assert_eq!(upper.as_str(), HEAD);
        for value in ["", "aaaaaaa", &format!("{HEAD}a"), &HEAD.replace('a', "z")] {
            assert!(CommitSha::parse(value).is_none(), "{value}");
        }
    }

    fn coderabbit_body(actionable: u32, base: &str, head: &str) -> String {
        format!(
            "**Actionable comments posted: {actionable}**\n\n\
             <details>\n<summary>Commits</summary>\n\n\
             Reviewing files that changed from the base of the PR and between `{base}` and `{head}`.\n\n\
             </details>"
        )
    }

    fn authored(author: &str, body: &str) -> ReviewComment {
        ReviewComment {
            id: 12,
            author: author.to_string(),
            body: body.to_string(),
            url: "https://github.com/owner/repository/pull/7#issuecomment-12".into(),
            created_at: "2026-09-06T00:00:00Z".into(),
        }
    }

    struct Housebot;

    impl ReviewSignal for Housebot {
        fn reviewer(&self) -> &str {
            "housebot"
        }

        fn required(&self) -> Confidence {
            Confidence { score: 7, scale: 7 }
        }

        fn identifies(&self, body: &str) -> bool {
            body.contains("Housebot verdict")
        }

        fn confidence(&self, body: &str) -> Option<Confidence> {
            body.contains("Housebot verdict: clean")
                .then_some(Confidence { score: 7, scale: 7 })
        }

        fn reviewed_commit(&self, body: &str) -> Option<CommitSha> {
            body.split_whitespace().find_map(CommitSha::parse)
        }
    }

    #[test]
    fn coderabbit_reads_its_own_comment_and_states_its_own_scale() {
        let body = coderabbit_body(0, OLDER, HEAD);
        assert!(CodeRabbit.identifies(&body));
        assert_eq!(
            CodeRabbit.confidence(&body),
            Some(Confidence { score: 1, scale: 1 })
        );
        assert_eq!(CodeRabbit.reviewed_commit(&body), CommitSha::parse(HEAD));

        let noisy = coderabbit_body(3, OLDER, HEAD);
        assert_eq!(
            CodeRabbit.confidence(&noisy),
            Some(Confidence { score: 0, scale: 1 }),
            "actionable comments are not a clean review"
        );
        assert!(!CodeRabbit.identifies("Nice work!"));
        assert!(
            CodeRabbit
                .reviewed_commit("Reviewing between aaaaaaa and bbbbbbb")
                .is_none(),
            "a short hex run is not a commit name"
        );
    }

    #[test]
    fn coderabbit_blocks_when_it_reviewed_an_older_commit() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![
                        comment(&summary_body(5, HEAD)),
                        authored(CODERABBIT_REVIEWER, &coderabbit_body(0, HEAD, OLDER)),
                    ],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!assessment.ready);
        assert!(!assessment.review_current);
        assert_eq!(assessment.reviews.len(), 2);
        assert_eq!(
            assessment.reports(),
            vec![BlockerReport {
                code: BlockerCode::ReviewedCommitStale,
                detail: "coderabbitai reviewed bbbbbbb; head is aaaaaaa".into(),
            }],
            "one bot's current review does not excuse another's stale one"
        );
    }

    #[test]
    fn two_recognized_bots_at_their_own_bars_are_ready_together() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![
                        comment(&summary_body(5, HEAD)),
                        authored(
                            &format!("{CODERABBIT_REVIEWER}{BOT_SUFFIX}"),
                            &coderabbit_body(0, OLDER, HEAD),
                        ),
                    ],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(assessment.ready, "{:?}", assessment.reports());
        assert_eq!(
            assessment
                .reviews
                .iter()
                .map(|review| review.reviewer.as_str())
                .collect::<Vec<_>>(),
            vec![CODERABBIT_REVIEWER, GREPTILE_REVIEWER],
            "a bot account is the same reviewer with or without its [bot] suffix"
        );
    }

    #[test]
    fn a_perfect_score_on_one_scale_never_stands_in_for_another() {
        assert!(
            !Confidence { score: 1, scale: 1 }.satisfies(Greptile.required()),
            "a clean 1/1 must not clear a 5/5 bar"
        );
        assert!(
            !Confidence { score: 5, scale: 5 }.satisfies(CodeRabbit.required()),
            "a 5/5 must not clear a 1/1 bar"
        );
        assert!(
            !Confidence {
                score: 10,
                scale: 10
            }
            .satisfies(Greptile.required()),
            "a 10/10 must not clear a 5/5 bar"
        );

        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![
                        comment(&summary_body(5, HEAD)),
                        authored(
                            CODERABBIT_REVIEWER,
                            &format!(
                                "Confidence Score: 10/10\n{}",
                                coderabbit_body(0, OLDER, HEAD)
                            ),
                        ),
                    ],
                },
                ..green_pull()
            },
            &ReviewSignals::of([SignalKind::Greptile]),
        );
        assert!(
            assessment.ready,
            "a bot outside the recognised set is not review evidence at all"
        );
    }

    #[test]
    fn the_recognized_set_decides_which_comments_are_review_evidence() {
        let comments = CommentEvidence {
            complete: true,
            comments: vec![comment(&summary_body(5, HEAD))],
        };

        let recognized = assess(
            PullEvidence {
                comments: comments.clone(),
                ..green_pull()
            },
            &ReviewSignals::of([SignalKind::Greptile]),
        );
        assert!(recognized.ready);

        let elsewhere = assess(
            PullEvidence {
                comments: comments.clone(),
                ..green_pull()
            },
            &ReviewSignals::of([SignalKind::CodeRabbit]),
        );
        assert!(!elsewhere.ready);
        assert_eq!(
            elsewhere.reports(),
            vec![BlockerReport {
                code: BlockerCode::ReviewMissing,
                detail: "coderabbitai summary missing".into(),
            }]
        );

        let none = assess(
            PullEvidence {
                comments,
                ..green_pull()
            },
            &ReviewSignals::empty(),
        );
        assert!(
            !none.ready,
            "an empty set can never establish review evidence"
        );
        assert_eq!(
            none.reports()[0].detail,
            "No review signal is recognised, so no review evidence can be established"
        );
    }

    #[test]
    fn any_bot_account_names_a_signal_and_only_a_blank_name_is_rejected() {
        assert_eq!(
            ReviewSignals::select(&["greptile", "CodeRabbit", "greptile-apps"])
                .unwrap()
                .reviewers(),
            vec![GREPTILE_REVIEWER, CODERABBIT_REVIEWER],
            "names resolve to signals and repeats collapse"
        );
        assert_eq!(
            SignalKind::parse("coderabbitai[bot]"),
            Some(SignalKind::CodeRabbit)
        );
        assert_eq!(
            SignalKind::parse("Appwrite[bot]"),
            Some(SignalKind::Other("appwrite".to_string())),
            "a bot this build has no vendor knowledge of still names a signal"
        );
        assert!(
            !SignalKind::parse("appwrite").expect("a bot").is_known(),
            "and says so, so nothing claims vendor knowledge it lacks"
        );
        assert_eq!(
            SignalKind::parse("appwrite").and_then(|kind| kind.trigger_command()),
            None,
            "a bot whose command is unknown is waited for, never prodded"
        );
        assert_eq!(
            ReviewSignals::select(&["appwrite"]).unwrap().reviewers(),
            vec!["appwrite"],
        );
        assert!(SignalKind::parse("   ").is_none());
        let error = ReviewSignals::select(&[" "]).unwrap_err();
        assert!(error.contains("blank"), "{error}");
        assert_eq!(ReviewSignals::recognized().len(), SignalKind::known().len());
        assert!(ReviewSignals::recognized().open());
        assert!(
            !ReviewSignals::select(&["appwrite"]).unwrap().open(),
            "a deployment that named its bots waits for those and no others"
        );
        assert!(ReviewSignals::empty().is_empty());
    }

    #[test]
    fn a_bot_this_build_never_heard_of_is_read_from_its_own_summary() {
        let summary = |body: &str| ReviewComment {
            id: 71,
            author: "appwrite[bot]".to_string(),
            body: body.to_string(),
            url: "https://github.com/acme/shop/pull/4#issuecomment-71".to_string(),
            created_at: "2026-09-20T10:00:00Z".to_string(),
        };
        let graded = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![summary(&format!(
                        "Confidence Score: 5/5\n\nLast reviewed commit: {HEAD}"
                    ))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(
            graded.ready,
            "a bot nobody configured reviewed the head: {:?}",
            graded.reports()
        );
        assert_eq!(graded.reviews.len(), 1);
        assert_eq!(graded.reviews[0].reviewer, "appwrite");
        assert!(
            graded.reviews[0].url.is_some(),
            "the summary it was read from is linked"
        );

        let reserved = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![summary(&format!(
                        "Confidence Score: 3/5\n\nLast reviewed commit: {HEAD}"
                    ))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!reserved.ready);
        assert_eq!(
            reserved.reports()[0].detail,
            "appwrite confidence 3/5; 5/5 required",
            "full marks on its own scale, since which of its scores means ship it is its own business"
        );

        let person = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![ReviewComment {
                        author: "a-colleague".to_string(),
                        ..summary(&format!(
                            "Confidence Score: 5/5\n\nLast reviewed commit: {HEAD}"
                        ))
                    }],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(
            !person.ready,
            "a comment GitHub did not mark a bot's is never a bot review"
        );
        assert_eq!(person.reports()[0].code, BlockerCode::ReviewMissing);

        let unreadable = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![summary(&format!("Last reviewed commit: {HEAD}"))],
                },
                ..green_pull()
            },
            &ReviewSignals::recognized(),
        );
        assert!(!unreadable.ready);
        assert_eq!(
            unreadable.reports()[0].code,
            BlockerCode::ConfidenceUnreadable,
            "a summary without a score is a named blocker, never a pass"
        );
    }

    #[test]
    fn the_assessment_holds_no_vendor_knowledge() {
        let signals = ReviewSignals::custom(vec![Box::new(Housebot)]);
        let clean = authored("housebot", &format!("Housebot verdict: clean {HEAD}"));
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![clean, comment(&summary_body(1, OLDER))],
                },
                ..green_pull()
            },
            &signals,
        );
        assert!(
            assessment.ready,
            "a signal the module has never heard of judges itself: {:?}",
            assessment.reports()
        );
        assert_eq!(assessment.reviews.len(), 1);
        assert_eq!(assessment.reviews[0].reviewer, "housebot");

        let dirty = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![authored(
                        "housebot",
                        &format!("Housebot verdict: findings {HEAD}"),
                    )],
                },
                ..green_pull()
            },
            &signals,
        );
        assert!(!dirty.ready);
        assert_eq!(
            dirty.reports()[0].code,
            BlockerCode::ConfidenceUnreadable,
            "the bar and the scale both come from the signal"
        );
    }
}

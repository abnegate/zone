//! Promotion of repeated answers to standing instructions.
//!
//! Questions a workspace keeps asking are clustered by the embeddings zone already
//! stores for every chat message. When a cluster clears every bar in [`PromotionPolicy`]
//! its agreed answer is written to the knowledge store as a standing instruction.
//!
//! A wrongly promoted instruction silently steers every later answer in the workspace,
//! so the bars are deliberately high: the question must recur across several separate
//! chats, the cluster must stay tight around its first member, and the answers given
//! must agree with each other. Every promotion records its provenance and can be
//! retired with [`crate::db::knowledge::retire_standing_instruction`].

use chrono::{Duration as CalendarDuration, NaiveDateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

use crate::db::{DbResult, knowledge};
use crate::state::AppState;

const PROMOTION_INTERVAL_SECONDS: u64 = 6 * 60 * 60;
const MINIMUM_OCCURRENCES: usize = 4;
const MINIMUM_DISTINCT_CHATS: usize = 3;
const MINIMUM_QUESTION_SIMILARITY: f32 = 0.88;
const ANSWER_SIMILARITY: f32 = 0.85;
const MINIMUM_ANSWER_AGREEMENT: f32 = 0.6;
const MINIMUM_ANSWERS_COMPARED: usize = 2;
const MINIMUM_QUESTION_CHARACTERS: usize = 12;
const MINIMUM_ANSWER_CHARACTERS: usize = 40;
const MAXIMUM_ANSWER_CHARACTERS: usize = 4_000;
const MAXIMUM_PROMOTIONS_PER_WORKSPACE: usize = 20;
const MAXIMUM_EXCHANGES_PER_WORKSPACE: i64 = 2_000;
const LOOKBACK_DAYS: i64 = 90;
const FINGERPRINT_CHARACTERS: usize = 32;
const MAXIMUM_TITLE_CHARACTERS: usize = 110;

/// Every bar a recurring question must clear before its answer becomes standing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromotionPolicy {
    /// How many times the question must recur.
    pub minimum_occurrences: usize,
    /// How many separate chats it must recur across, so one long thread cannot promote.
    pub minimum_distinct_chats: usize,
    /// Cosine similarity every clustered question must hold to the cluster's first member.
    pub minimum_question_similarity: f32,
    /// Cosine similarity at which two answers count as saying the same thing.
    pub answer_similarity: f32,
    /// Fraction of the cluster's answers that must agree with the one being promoted.
    pub minimum_answer_agreement: f32,
    pub minimum_question_characters: usize,
    pub minimum_answer_characters: usize,
    pub maximum_answer_characters: usize,
    pub maximum_promotions_per_workspace: usize,
    pub maximum_exchanges_per_workspace: i64,
    pub lookback_days: i64,
}

impl Default for PromotionPolicy {
    fn default() -> Self {
        Self {
            minimum_occurrences: MINIMUM_OCCURRENCES,
            minimum_distinct_chats: MINIMUM_DISTINCT_CHATS,
            minimum_question_similarity: MINIMUM_QUESTION_SIMILARITY,
            answer_similarity: ANSWER_SIMILARITY,
            minimum_answer_agreement: MINIMUM_ANSWER_AGREEMENT,
            minimum_question_characters: MINIMUM_QUESTION_CHARACTERS,
            minimum_answer_characters: MINIMUM_ANSWER_CHARACTERS,
            maximum_answer_characters: MAXIMUM_ANSWER_CHARACTERS,
            maximum_promotions_per_workspace: MAXIMUM_PROMOTIONS_PER_WORKSPACE,
            maximum_exchanges_per_workspace: MAXIMUM_EXCHANGES_PER_WORKSPACE,
            lookback_days: LOOKBACK_DAYS,
        }
    }
}

/// One question and the answer it received, with the embeddings zone already stored.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    pub question_id: Uuid,
    pub chat_id: Uuid,
    pub question: String,
    pub answer: String,
    pub question_embedding: Vec<f32>,
    pub answer_embedding: Option<Vec<f32>>,
    pub asked_at: NaiveDateTime,
}

/// A group of questions that mean the same thing.
#[derive(Debug, Clone)]
pub struct QuestionCluster {
    exchanges: Vec<Exchange>,
    centroid: Vec<f32>,
}

impl QuestionCluster {
    fn new(exchange: Exchange) -> Self {
        let centroid = normalized(&exchange.question_embedding);
        Self {
            exchanges: vec![exchange],
            centroid,
        }
    }

    fn admit(&mut self, exchange: Exchange) {
        for (total, value) in self
            .centroid
            .iter_mut()
            .zip(normalized(&exchange.question_embedding))
        {
            *total += value;
        }
        self.exchanges.push(exchange);
    }

    /// Similarity to both the cluster's first member and its centroid. Requiring both
    /// stops a chain of small steps from drifting a cluster onto a different subject.
    fn affinity(&self, embedding: &[f32]) -> f32 {
        let to_leader = cosine_similarity(&self.leader().question_embedding, embedding);
        let to_centroid = cosine_similarity(&self.centroid, embedding);
        to_leader.min(to_centroid)
    }

    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    pub fn leader(&self) -> &Exchange {
        &self.exchanges[0]
    }

    pub fn occurrences(&self) -> usize {
        self.exchanges.len()
    }

    pub fn distinct_chats(&self) -> usize {
        self.exchanges
            .iter()
            .map(|exchange| exchange.chat_id)
            .collect::<HashSet<_>>()
            .len()
    }

    /// The weakest link: how close the least similar member sits to the first one.
    pub fn cohesion(&self) -> f32 {
        let leader = &self.leader().question_embedding;
        self.exchanges
            .iter()
            .skip(1)
            .map(|exchange| cosine_similarity(leader, &exchange.question_embedding))
            .fold(1.0, f32::min)
    }
}

/// A cluster that cleared every bar, with the evidence behind the decision.
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionCandidate {
    pub fingerprint: String,
    pub question: String,
    pub answer: String,
    pub occurrences: usize,
    pub distinct_chats: usize,
    pub cohesion: f32,
    pub agreement: f32,
    pub first_seen: NaiveDateTime,
    pub last_seen: NaiveDateTime,
}

/// Counts from one workspace pass, for logging and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromotionReport {
    pub exchanges: usize,
    pub candidates: usize,
    pub created: usize,
    pub superseded: usize,
    pub unchanged: usize,
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }

    let (dot, left_norm, right_norm) = left.iter().zip(right.iter()).fold(
        (0.0f32, 0.0f32, 0.0f32),
        |(dot, left_norm, right_norm), (&left, &right)| {
            (
                dot + left * right,
                left_norm + left * left,
                right_norm + right * right,
            )
        },
    );

    let magnitude = left_norm.sqrt() * right_norm.sqrt();
    if magnitude == 0.0 || !magnitude.is_finite() {
        return 0.0;
    }

    dot / magnitude
}

fn normalized(vector: &[f32]) -> Vec<f32> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return vector.to_vec();
    }
    vector.iter().map(|value| value / norm).collect()
}

/// Case-folded, whitespace-collapsed, punctuation-trimmed text for stable identity.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|character: char| {
                character.is_ascii_punctuation() && character != '_'
            })
            .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Identity of a cluster, taken from its earliest question so that later occurrences
/// join the same standing instruction instead of creating a second one.
pub fn fingerprint(question: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalize(question).as_bytes());
    hex::encode(hasher.finalize())
        .chars()
        .take(FINGERPRINT_CHARACTERS)
        .collect()
}

fn summarize(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(limit).collect();
    format!("{}…", truncated.trim_end())
}

/// Group exchanges whose questions mean the same thing.
pub fn cluster_questions(exchanges: &[Exchange], policy: &PromotionPolicy) -> Vec<QuestionCluster> {
    let mut ordered: Vec<&Exchange> = exchanges.iter().collect();
    ordered.sort_by(|left, right| {
        left.asked_at
            .cmp(&right.asked_at)
            .then(left.question_id.cmp(&right.question_id))
    });

    let mut clusters: Vec<QuestionCluster> = Vec::new();
    for exchange in ordered {
        let best = clusters
            .iter()
            .map(|cluster| cluster.affinity(&exchange.question_embedding))
            .enumerate()
            .filter(|(_, affinity)| *affinity >= policy.minimum_question_similarity)
            .max_by(|(_, left), (_, right)| left.total_cmp(right));

        match best {
            Some((index, _)) => clusters[index].admit(exchange.clone()),
            None => clusters.push(QuestionCluster::new(exchange.clone())),
        }
    }

    clusters
}

fn answers_agree(left: &Exchange, right: &Exchange, policy: &PromotionPolicy) -> bool {
    match (&left.answer_embedding, &right.answer_embedding) {
        (Some(left_vector), Some(right_vector)) => {
            cosine_similarity(left_vector, right_vector) >= policy.answer_similarity
        }
        _ => normalize(&left.answer) == normalize(&right.answer),
    }
}

struct AgreedAnswer {
    text: String,
    agreement: f32,
    asked_at: NaiveDateTime,
}

/// The answer the workspace actually settled on: the one most of the others agree with,
/// preferring the most recent when several are equally agreed.
fn select_answer(cluster: &QuestionCluster, policy: &PromotionPolicy) -> Option<AgreedAnswer> {
    let candidates: Vec<&Exchange> = cluster
        .exchanges()
        .iter()
        .filter(|exchange| {
            let length = exchange.answer.trim().chars().count();
            length >= policy.minimum_answer_characters && length <= policy.maximum_answer_characters
        })
        .collect();

    if candidates.len() < policy.minimum_occurrences.max(MINIMUM_ANSWERS_COMPARED) {
        return None;
    }

    let comparisons = (candidates.len() - 1) as f32;
    let mut best: Option<AgreedAnswer> = None;

    for (index, candidate) in candidates.iter().enumerate() {
        let agreeing = candidates
            .iter()
            .enumerate()
            .filter(|(other, exchange)| {
                *other != index && answers_agree(candidate, exchange, policy)
            })
            .count();
        let agreement = agreeing as f32 / comparisons;

        let improves = best.as_ref().is_none_or(|current| {
            agreement > current.agreement
                || (agreement == current.agreement && candidate.asked_at > current.asked_at)
        });

        if improves {
            best = Some(AgreedAnswer {
                text: candidate.answer.trim().to_string(),
                agreement,
                asked_at: candidate.asked_at,
            });
        }
    }

    best.filter(|answer| answer.agreement >= policy.minimum_answer_agreement)
}

/// Decide whether one cluster earns a standing instruction.
pub fn evaluate_cluster(
    cluster: &QuestionCluster,
    policy: &PromotionPolicy,
) -> Option<PromotionCandidate> {
    if cluster.occurrences() < policy.minimum_occurrences
        || cluster.distinct_chats() < policy.minimum_distinct_chats
        || cluster.cohesion() < policy.minimum_question_similarity
    {
        return None;
    }

    let answer = select_answer(cluster, policy)?;
    let first_seen = cluster.leader().asked_at;
    let last_seen = cluster
        .exchanges()
        .iter()
        .map(|exchange| exchange.asked_at)
        .max()
        .unwrap_or(first_seen);

    Some(PromotionCandidate {
        fingerprint: fingerprint(&cluster.leader().question),
        question: cluster.leader().question.trim().to_string(),
        answer: answer.text,
        occurrences: cluster.occurrences(),
        distinct_chats: cluster.distinct_chats(),
        cohesion: cluster.cohesion(),
        agreement: answer.agreement,
        first_seen,
        last_seen,
    })
}

/// The full decision, pure over in-memory exchanges: cluster, judge, rank, cap.
pub fn promotion_candidates(
    exchanges: &[Exchange],
    policy: &PromotionPolicy,
) -> Vec<PromotionCandidate> {
    let mut candidates: Vec<PromotionCandidate> = cluster_questions(exchanges, policy)
        .iter()
        .filter_map(|cluster| evaluate_cluster(cluster, policy))
        .collect();

    candidates.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then(right.last_seen.cmp(&left.last_seen))
            .then(left.fingerprint.cmp(&right.fingerprint))
    });
    candidates.truncate(policy.maximum_promotions_per_workspace);
    candidates
}

/// The knowledge row a candidate becomes. Deterministic, so an unchanged cluster
/// produces a byte-identical entry and the upsert reports no change.
pub fn standing_instruction(
    workspace_id: Uuid,
    candidate: &PromotionCandidate,
) -> knowledge::StandingInstruction {
    knowledge::StandingInstruction {
        workspace_id,
        title: format!(
            "Repeated answer: {}",
            summarize(&candidate.question, MAXIMUM_TITLE_CHARACTERS)
        ),
        content: candidate.answer.clone(),
        provenance: knowledge::PromotionProvenance {
            fingerprint: candidate.fingerprint.clone(),
            occurrences: candidate.occurrences,
            distinct_chats: candidate.distinct_chats,
            last_confirmed: candidate.last_seen.date(),
        },
    }
}

fn parse_vector(text: &str) -> Option<Vec<f32>> {
    let body = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    if body.trim().is_empty() {
        return None;
    }
    let vector: Option<Vec<f32>> = body
        .split(',')
        .map(|value| value.trim().parse::<f32>().ok())
        .collect();
    vector.filter(|vector| vector.iter().all(|value| value.is_finite()))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ExchangeRow {
    question_id: Uuid,
    chat_id: Uuid,
    question: String,
    answer: String,
    asked_at: NaiveDateTime,
    question_vector: String,
    answer_vector: Option<String>,
}

impl ExchangeRow {
    fn into_exchange(self) -> Option<Exchange> {
        Some(Exchange {
            question_id: self.question_id,
            chat_id: self.chat_id,
            question: self.question,
            answer: self.answer,
            question_embedding: parse_vector(&self.question_vector)?,
            answer_embedding: self.answer_vector.as_deref().and_then(parse_vector),
            asked_at: self.asked_at,
        })
    }
}

/// Workspaces worth scanning: those with a user message inside the lookback window.
async fn active_workspaces(pool: &PgPool, since: NaiveDateTime) -> DbResult<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT DISTINCT chat.workspace_id
        FROM messages message
        JOIN chats chat ON chat.id = message.chat_id
        JOIN workspaces workspace ON workspace.id = chat.workspace_id
        WHERE message.created_at >= $1
          AND message.role = 'user'
          AND workspace.is_active IS NOT FALSE
        "#,
    )
    .bind(since)
    .fetch_all(pool)
    .await
}

/// Each user question in the workspace paired with the assistant reply it drew,
/// skipping any question whose reply is separated from it by another question.
///
/// The cap keeps the most recent exchanges, so a busy workspace keeps learning rather
/// than freezing on its oldest history.
pub async fn load_exchanges(
    pool: &PgPool,
    workspace_id: Uuid,
    policy: &PromotionPolicy,
) -> DbResult<Vec<Exchange>> {
    let since = Utc::now().naive_utc() - CalendarDuration::days(policy.lookback_days);

    let rows: Vec<ExchangeRow> = sqlx::query_as(
        r#"
        SELECT
            question.id AS question_id,
            question.chat_id AS chat_id,
            question.content AS question,
            reply.content AS answer,
            question.created_at AS asked_at,
            question_embedding.vector::text AS question_vector,
            answer_embedding.vector::text AS answer_vector
        FROM messages question
        JOIN chats chat ON chat.id = question.chat_id
        JOIN message_embeddings question_embedding
            ON question_embedding.message_id = question.id
        LEFT JOIN LATERAL (
            SELECT later.created_at, later.id
            FROM messages later
            WHERE later.chat_id = question.chat_id
              AND later.role = 'user'
              AND (later.created_at, later.id) > (question.created_at, question.id)
            ORDER BY later.created_at, later.id
            LIMIT 1
        ) next_question ON TRUE
        JOIN LATERAL (
            SELECT candidate.id, candidate.content
            FROM messages candidate
            WHERE candidate.chat_id = question.chat_id
              AND candidate.role = 'assistant'
              AND (candidate.created_at, candidate.id) > (question.created_at, question.id)
              AND (next_question.created_at IS NULL
                   OR (candidate.created_at, candidate.id) < (next_question.created_at, next_question.id))
            ORDER BY candidate.created_at, candidate.id
            LIMIT 1
        ) reply ON TRUE
        LEFT JOIN message_embeddings answer_embedding
            ON answer_embedding.message_id = reply.id
        WHERE chat.workspace_id = $1
          AND question.role = 'user'
          AND question.created_at >= $2
          AND char_length(question.content) >= $3
        ORDER BY question.created_at DESC, question.id DESC
        LIMIT $4
        "#,
    )
    .bind(workspace_id)
    .bind(since)
    .bind(policy.minimum_question_characters as i32)
    .bind(policy.maximum_exchanges_per_workspace)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(ExchangeRow::into_exchange)
        .collect())
}

/// Scan one workspace and write every qualifying answer to the knowledge store.
pub async fn promote_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    policy: &PromotionPolicy,
) -> DbResult<PromotionReport> {
    let exchanges = load_exchanges(pool, workspace_id, policy).await?;
    let candidates = promotion_candidates(&exchanges, policy);

    let mut report = PromotionReport {
        exchanges: exchanges.len(),
        candidates: candidates.len(),
        ..PromotionReport::default()
    };

    for candidate in &candidates {
        let instruction = standing_instruction(workspace_id, candidate);
        let (id, outcome) = knowledge::upsert_standing_instruction(pool, &instruction).await?;

        match outcome {
            knowledge::StandingInstructionOutcome::Created => {
                report.created += 1;
                tracing::info!(
                    %workspace_id,
                    knowledge_entry_id = %id,
                    occurrences = candidate.occurrences,
                    distinct_chats = candidate.distinct_chats,
                    cohesion = candidate.cohesion,
                    agreement = candidate.agreement,
                    question = %summarize(&candidate.question, MAXIMUM_TITLE_CHARACTERS),
                    "Promoted a repeated answer to a standing instruction"
                );
            }
            knowledge::StandingInstructionOutcome::Superseded => {
                report.superseded += 1;
                tracing::info!(
                    %workspace_id,
                    knowledge_entry_id = %id,
                    occurrences = candidate.occurrences,
                    "Superseded a standing instruction with an updated answer"
                );
            }
            knowledge::StandingInstructionOutcome::Unchanged => report.unchanged += 1,
        }
    }

    Ok(report)
}

async fn run_cycle(state: &AppState, policy: &PromotionPolicy) -> DbResult<()> {
    let since = Utc::now().naive_utc() - CalendarDuration::days(policy.lookback_days);
    let workspaces = active_workspaces(state.db(), since).await?;

    for workspace_id in workspaces {
        match promote_workspace(state.db(), workspace_id, policy).await {
            Ok(report) => tracing::debug!(
                %workspace_id,
                exchanges = report.exchanges,
                created = report.created,
                superseded = report.superseded,
                "Standing instruction scan finished"
            ),
            Err(error) => tracing::warn!(
                %workspace_id,
                %error,
                "Standing instruction scan failed; retrying next cycle"
            ),
        }
    }

    Ok(())
}

/// Run the promoter on an interval. The first pass waits one interval so server
/// startup is not competing with a full workspace scan.
pub fn spawn(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let policy = PromotionPolicy::default();
        let mut interval = tokio::time::interval(Duration::from_secs(PROMOTION_INTERVAL_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;

        loop {
            interval.tick().await;
            if let Err(error) = run_cycle(&state, &policy).await {
                tracing::warn!(%error, "Standing instruction promotion cycle failed");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    const EMBEDDING_DIMENSION: usize = 4;
    const TESTS_QUESTION: [f32; 4] = [1.0, 0.05, 0.0, 0.0];
    const TESTS_QUESTION_PARAPHRASE: [f32; 4] = [0.98, 0.14, 0.02, 0.0];
    const BILLING_QUESTION: [f32; 4] = [0.0, 0.0, 1.0, 0.1];
    const TESTS_ANSWER: [f32; 4] = [0.0, 1.0, 0.0, 0.05];
    const CONTRADICTING_ANSWER: [f32; 4] = [0.0, 0.0, 0.1, 1.0];

    fn moment(day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap()
    }

    fn exchange(
        index: u128,
        chat: u128,
        question: &str,
        question_embedding: [f32; 4],
        answer: &str,
        answer_embedding: [f32; 4],
    ) -> Exchange {
        Exchange {
            question_id: Uuid::from_u128(index),
            chat_id: Uuid::from_u128(chat),
            question: question.to_string(),
            answer: answer.to_string(),
            question_embedding: question_embedding.to_vec(),
            answer_embedding: Some(answer_embedding.to_vec()),
            asked_at: moment(index as u32),
        }
    }

    const RUN_TESTS_ANSWER: &str =
        "Run cargo test from the runner directory; the workspace tests need no database.";

    fn recurring_question() -> Vec<Exchange> {
        vec![
            exchange(
                1,
                101,
                "How do I run the tests?",
                TESTS_QUESTION,
                RUN_TESTS_ANSWER,
                TESTS_ANSWER,
            ),
            exchange(
                2,
                102,
                "What command runs the test suite?",
                TESTS_QUESTION_PARAPHRASE,
                RUN_TESTS_ANSWER,
                TESTS_ANSWER,
            ),
            exchange(
                3,
                103,
                "How are the tests run here?",
                TESTS_QUESTION_PARAPHRASE,
                RUN_TESTS_ANSWER,
                TESTS_ANSWER,
            ),
            exchange(
                4,
                104,
                "Remind me how to run the tests",
                TESTS_QUESTION,
                RUN_TESTS_ANSWER,
                TESTS_ANSWER,
            ),
        ]
    }

    #[test]
    fn empty_workspace_promotes_nothing() {
        let candidates = promotion_candidates(&[], &PromotionPolicy::default());
        assert!(
            candidates.is_empty(),
            "a workspace with no exchanges must promote nothing"
        );
    }

    #[test]
    fn single_exchange_promotes_nothing() {
        let exchanges = vec![recurring_question()[0].clone()];
        let candidates = promotion_candidates(&exchanges, &PromotionPolicy::default());
        assert!(
            candidates.is_empty(),
            "one exchange is never enough evidence to promote"
        );
    }

    #[test]
    fn cluster_below_occurrence_threshold_is_not_promoted() {
        let mut exchanges = recurring_question();
        exchanges.truncate(3);

        let policy = PromotionPolicy::default();
        assert_eq!(policy.minimum_occurrences, 4);
        assert!(
            promotion_candidates(&exchanges, &policy).is_empty(),
            "three occurrences must stay below the four-occurrence bar"
        );
    }

    #[test]
    fn cluster_above_occurrence_threshold_is_promoted() {
        let candidates = promotion_candidates(&recurring_question(), &PromotionPolicy::default());

        assert_eq!(candidates.len(), 1, "the repeated question should promote");
        let candidate = &candidates[0];
        assert_eq!(candidate.answer, RUN_TESTS_ANSWER);
        assert_eq!(candidate.occurrences, 4);
        assert_eq!(candidate.distinct_chats, 4);
        assert_eq!(candidate.first_seen, moment(1));
        assert_eq!(candidate.last_seen, moment(4));
    }

    #[test]
    fn repetition_inside_one_chat_is_not_promoted() {
        let exchanges: Vec<Exchange> = recurring_question()
            .into_iter()
            .enumerate()
            .map(|(index, mut exchange)| {
                exchange.chat_id = Uuid::from_u128(if index == 0 { 101 } else { 102 });
                exchange
            })
            .collect();

        assert!(
            promotion_candidates(&exchanges, &PromotionPolicy::default()).is_empty(),
            "a question repeated within two chats must not clear the three-chat bar"
        );
    }

    #[test]
    fn semantically_similar_questions_cluster_together() {
        let clusters = cluster_questions(&recurring_question(), &PromotionPolicy::default());
        assert_eq!(clusters.len(), 1, "paraphrases belong in one cluster");
        assert_eq!(clusters[0].occurrences(), 4);
        assert!(clusters[0].cohesion() >= PromotionPolicy::default().minimum_question_similarity);
    }

    #[test]
    fn unrelated_questions_stay_in_separate_clusters() {
        let mut exchanges = recurring_question();
        exchanges.push(exchange(
            5,
            105,
            "Who owns the billing integration?",
            BILLING_QUESTION,
            "Billing is owned by the platform team.",
            CONTRADICTING_ANSWER,
        ));

        let clusters = cluster_questions(&exchanges, &PromotionPolicy::default());
        assert_eq!(
            clusters.len(),
            2,
            "an unrelated question must not join the cluster"
        );
        assert_eq!(clusters[1].occurrences(), 1);
    }

    #[test]
    fn promotion_is_idempotent() {
        let workspace = Uuid::from_u128(9);
        let policy = PromotionPolicy::default();
        let exchanges = recurring_question();

        let first = promotion_candidates(&exchanges, &policy);
        let second = promotion_candidates(&exchanges, &policy);
        assert_eq!(first, second, "the same input must decide the same way");

        let first_instruction = standing_instruction(workspace, &first[0]);
        let second_instruction = standing_instruction(workspace, &second[0]);
        assert_eq!(
            first_instruction, second_instruction,
            "an unchanged cluster must produce an identical knowledge entry"
        );
    }

    #[test]
    fn updated_answer_supersedes_rather_than_duplicating() {
        let workspace = Uuid::from_u128(9);
        let policy = PromotionPolicy::default();
        const UPDATED_ANSWER: &str =
            "Run cargo test -p zone_server --lib; the integration tests need a database.";

        let original = promotion_candidates(&recurring_question(), &policy);

        let mut later = recurring_question();
        for (offset, exchange) in later.iter_mut().enumerate() {
            exchange.answer = UPDATED_ANSWER.to_string();
            exchange.asked_at = moment(10 + offset as u32);
        }
        let updated = promotion_candidates(&later, &policy);

        assert_eq!(
            original[0].fingerprint, updated[0].fingerprint,
            "the identity key must survive an answer change so the row is superseded"
        );
        assert_ne!(original[0].answer, updated[0].answer);

        let original_instruction = standing_instruction(workspace, &original[0]);
        let updated_instruction = standing_instruction(workspace, &updated[0]);
        assert_eq!(
            original_instruction.provenance.fingerprint, updated_instruction.provenance.fingerprint,
            "both writes address the same knowledge row"
        );
        assert_eq!(updated_instruction.content, UPDATED_ANSWER);
    }

    #[test]
    fn disagreeing_answers_are_not_promoted() {
        let mut exchanges = recurring_question();
        for (index, exchange) in exchanges.iter_mut().enumerate() {
            let mut embedding = vec![0.0; EMBEDDING_DIMENSION];
            embedding[index] = 1.0;
            exchange.answer =
                format!("Answer variant {index} that shares no wording with any other");
            exchange.answer_embedding = Some(embedding);
        }

        assert!(
            promotion_candidates(&exchanges, &PromotionPolicy::default()).is_empty(),
            "answers that contradict each other must never become standing instructions"
        );
    }

    #[test]
    fn answers_below_the_length_floor_are_not_promoted() {
        let exchanges: Vec<Exchange> = recurring_question()
            .into_iter()
            .map(|mut exchange| {
                exchange.answer = "Yes.".to_string();
                exchange
            })
            .collect();

        assert!(
            promotion_candidates(&exchanges, &PromotionPolicy::default()).is_empty(),
            "a one-word answer carries no standing instruction"
        );
    }

    #[test]
    fn answer_agreement_survives_one_outlier() {
        let mut exchanges = recurring_question();
        exchanges.push(exchange(
            5,
            105,
            "How do I run the tests again?",
            TESTS_QUESTION,
            "Ask the platform team to run them for you in the shared environment.",
            CONTRADICTING_ANSWER,
        ));

        let candidates = promotion_candidates(&exchanges, &PromotionPolicy::default());
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].answer, RUN_TESTS_ANSWER,
            "the answer the workspace agrees on must win over a lone outlier"
        );
        assert!(candidates[0].agreement >= 0.6);
    }

    #[test]
    fn candidates_are_capped_per_workspace() {
        let policy = PromotionPolicy {
            maximum_promotions_per_workspace: 1,
            ..PromotionPolicy::default()
        };

        let mut exchanges = recurring_question();
        for index in 0..4u128 {
            exchanges.push(exchange(
                20 + index,
                200 + index,
                "Who owns the billing integration?",
                BILLING_QUESTION,
                "Billing is owned by the platform team and changes go through them.",
                CONTRADICTING_ANSWER,
            ));
        }

        assert_eq!(promotion_candidates(&exchanges, &policy).len(), 1);
    }

    #[test]
    fn standing_instruction_records_its_provenance() {
        let workspace = Uuid::from_u128(9);
        let candidates = promotion_candidates(&recurring_question(), &PromotionPolicy::default());
        let instruction = standing_instruction(workspace, &candidates[0]);

        assert_eq!(instruction.workspace_id, workspace);
        assert!(instruction.title.starts_with("Repeated answer: "));
        assert_eq!(instruction.content, RUN_TESTS_ANSWER);
        assert_eq!(instruction.provenance.occurrences, 4);
        assert_eq!(instruction.provenance.distinct_chats, 4);
        assert_eq!(
            instruction.provenance.last_confirmed,
            NaiveDate::from_ymd_opt(2026, 9, 4).unwrap()
        );
    }

    #[test]
    fn cosine_similarity_handles_degenerate_input() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(
            cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]),
            0.0,
            "mismatched dimensions must never look similar"
        );
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn normalize_folds_case_punctuation_and_spacing() {
        assert_eq!(
            normalize("  How do I   run the TESTS? "),
            "how do i run the tests"
        );
        assert_eq!(
            normalize("How do I run the tests"),
            normalize("how do i run the tests?")
        );
        assert_eq!(normalize("   "), "");
    }

    #[test]
    fn fingerprint_is_stable_across_surface_differences() {
        assert_eq!(
            fingerprint("How do I run the tests?"),
            fingerprint("  how do i run the TESTS  ")
        );
        assert_ne!(
            fingerprint("How do I run the tests?"),
            fingerprint("Who owns billing?")
        );
        assert_eq!(fingerprint("anything").len(), FINGERPRINT_CHARACTERS);
    }

    #[test]
    fn summarize_collapses_and_truncates_on_character_boundaries() {
        assert_eq!(summarize("one  two\nthree", 40), "one two three");
        let summary = summarize("ünïcödé question repeated many times over", 10);
        assert_eq!(summary.chars().count(), 11);
        assert!(summary.ends_with('…'));
    }

    #[test]
    fn parse_vector_reads_pgvector_text_and_rejects_junk() {
        assert_eq!(parse_vector("[0.5,-1,0]"), Some(vec![0.5, -1.0, 0.0]));
        assert_eq!(parse_vector(" [1, 2] "), Some(vec![1.0, 2.0]));
        assert_eq!(parse_vector("[]"), None);
        assert_eq!(parse_vector("0.5,1"), None);
        assert_eq!(parse_vector("[0.5,oops]"), None);
        assert_eq!(parse_vector("[NaN,1]"), None);
    }

    #[test]
    fn a_policy_with_no_thresholds_still_needs_two_answers_to_compare() {
        let policy = PromotionPolicy {
            minimum_occurrences: 0,
            minimum_distinct_chats: 0,
            minimum_answer_agreement: 0.0,
            ..PromotionPolicy::default()
        };

        let exchanges = vec![recurring_question()[0].clone()];
        assert!(
            promotion_candidates(&exchanges, &policy).is_empty(),
            "a lone answer has nothing to agree with, whatever the thresholds say"
        );
    }

    #[test]
    fn default_policy_keeps_a_high_bar() {
        let policy = PromotionPolicy::default();
        assert!(policy.minimum_occurrences >= 4);
        assert!(policy.minimum_distinct_chats >= 3);
        assert!(policy.minimum_question_similarity >= 0.85);
        assert!(policy.minimum_answer_agreement > 0.5);
    }
}

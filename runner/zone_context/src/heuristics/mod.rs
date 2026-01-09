//! Heuristic analysis pipeline
//!
//! Provides deep analysis of gathered content including:
//! - Entity extraction (people, dates, code references, URLs)
//! - Content categorization (topic, sentiment, priority)
//! - Quality metrics (freshness, reliability, density)
//! - Relevance scoring

mod categorization;
mod entities;
mod quality;
mod relevance;

pub use categorization::ContentCategorizer;
pub use entities::EntityExtractor;
pub use quality::QualityAnalyzer;
pub use relevance::RelevanceScorer;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Complete heuristic analysis of a content item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeuristicAnalysis {
    /// Content item ID this analysis is for
    pub content_item_id: Uuid,
    /// Extracted entities
    pub entities: ExtractedEntities,
    /// Content categorization
    pub categorization: ContentCategorization,
    /// Quality metrics
    pub quality: QualityScore,
    /// Relevance score (if computed against a query)
    pub relevance: Option<RelevanceScore>,
    /// When this analysis was performed
    pub analyzed_at: DateTime<Utc>,
}

/// Extracted entities from content
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedEntities {
    /// People mentioned (names, emails, usernames)
    pub people: Vec<PersonEntity>,
    /// Dates and temporal references
    pub dates: Vec<DateEntity>,
    /// Code references (functions, classes, files)
    pub code_refs: Vec<CodeReference>,
    /// URLs found
    pub urls: Vec<UrlEntity>,
    /// File paths mentioned
    pub file_paths: Vec<String>,
    /// Relationships between entities
    pub relationships: Vec<EntityRelationship>,
}

/// A person entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonEntity {
    /// Display name
    pub name: String,
    /// Email address
    pub email: Option<String>,
    /// Username (e.g., GitHub handle)
    pub username: Option<String>,
    /// Role if mentioned
    pub role: Option<String>,
}

/// A date/time entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateEntity {
    /// Raw text as found
    pub raw: String,
    /// Parsed datetime if successful
    pub parsed: Option<DateTime<Utc>>,
    /// Whether this is a relative reference ("next week")
    pub is_relative: bool,
    /// Whether this appears to be a deadline
    pub is_deadline: bool,
}

/// A code reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeReference {
    /// Name of the code element
    pub name: String,
    /// Type of code element
    pub kind: CodeRefKind,
    /// File path if known
    pub file_path: Option<String>,
    /// Line number if specified
    pub line_number: Option<u32>,
    /// Language if detected
    pub language: Option<String>,
}

/// Types of code references
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRefKind {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Module,
    Variable,
    Constant,
    Type,
    File,
    Other,
}

/// A URL entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlEntity {
    /// The URL
    pub url: String,
    /// Type of URL
    pub url_type: UrlType,
    /// Link text or title if available
    pub title: Option<String>,
}

/// Types of URLs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlType {
    Web,
    GitHub,
    GitLab,
    Jira,
    Confluence,
    Slack,
    Documentation,
    Api,
    Other,
}

/// A relationship between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationship {
    /// Source entity identifier
    pub from_entity: String,
    /// Target entity identifier
    pub to_entity: String,
    /// Type of relationship
    pub relationship_type: RelationshipType,
    /// Confidence in this relationship (0.0-1.0)
    pub confidence: f32,
}

/// Types of relationships between entities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipType {
    Mentions,
    AuthoredBy,
    AssignedTo,
    DependsOn,
    References,
    PartOf,
    SimilarTo,
    Blocks,
    FollowsUp,
}

/// Content categorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCategorization {
    /// Primary topic
    pub topic: Topic,
    /// Confidence in topic classification (0.0-1.0)
    pub topic_confidence: f32,
    /// Sentiment analysis result
    pub sentiment: Sentiment,
    /// Priority assessment
    pub priority: Priority,
    /// Actionability score
    pub actionability: ActionabilityScore,
    /// Tags derived from content
    pub tags: Vec<String>,
}

impl Default for ContentCategorization {
    fn default() -> Self {
        Self {
            topic: Topic::Other,
            topic_confidence: 0.0,
            sentiment: Sentiment::Neutral,
            priority: Priority::Medium,
            actionability: ActionabilityScore::default(),
            tags: Vec::new(),
        }
    }
}

/// Content topics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    Code,
    Documentation,
    Discussion,
    Planning,
    BugReport,
    FeatureRequest,
    Review,
    Meeting,
    Announcement,
    Question,
    Answer,
    Other,
}

/// Sentiment classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sentiment {
    Positive,
    Negative,
    Neutral,
    Mixed,
}

/// Priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

/// Actionability scoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionabilityScore {
    /// Overall actionability score (0.0-1.0)
    pub score: f32,
    /// Type of action detected
    pub action_type: ActionType,
    /// Detected action items
    pub action_items: Vec<String>,
    /// Whether this requires a response
    pub requires_response: bool,
}

/// Types of actions
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Task,
    Decision,
    Question,
    FeedbackRequest,
    #[default]
    Information,
    None,
}

/// Quality metrics for content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    /// Overall quality score (0.0-1.0)
    pub overall: f32,
    /// Freshness score based on age (0.0-1.0)
    pub freshness: f32,
    /// Source reliability score (0.0-1.0)
    pub reliability: f32,
    /// Information density (0.0-1.0)
    pub density: f32,
    /// Duplication score (0.0 = unique, 1.0 = exact duplicate)
    pub duplication: f32,
    /// ID of duplicate content if found
    pub duplicate_of: Option<Uuid>,
}

impl Default for QualityScore {
    fn default() -> Self {
        Self {
            overall: 0.5,
            freshness: 0.5,
            reliability: 0.5,
            density: 0.5,
            duplication: 0.0,
            duplicate_of: None,
        }
    }
}

/// Relevance score relative to a query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevanceScore {
    /// Query this was scored against
    pub query: String,
    /// Semantic similarity via embeddings (0.0-1.0)
    pub semantic_similarity: f32,
    /// Keyword overlap score (0.0-1.0)
    pub keyword_score: f32,
    /// Task relevance score (0.0-1.0)
    pub task_relevance: f32,
    /// Recency boost (0.0-1.0)
    pub recency_boost: f32,
    /// Combined relevance score (0.0-1.0)
    pub combined: f32,
}

impl RelevanceScore {
    /// Create a new relevance score
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            semantic_similarity: 0.0,
            keyword_score: 0.0,
            task_relevance: 0.0,
            recency_boost: 0.0,
            combined: 0.0,
        }
    }

    /// Calculate combined score with default weights
    pub fn calculate_combined(&mut self) {
        // Weights for combining scores
        const SEMANTIC_WEIGHT: f32 = 0.5;
        const KEYWORD_WEIGHT: f32 = 0.2;
        const TASK_WEIGHT: f32 = 0.2;
        const RECENCY_WEIGHT: f32 = 0.1;

        self.combined = (self.semantic_similarity * SEMANTIC_WEIGHT
            + self.keyword_score * KEYWORD_WEIGHT
            + self.task_relevance * TASK_WEIGHT
            + self.recency_boost * RECENCY_WEIGHT)
            .clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_score_default() {
        let score = QualityScore::default();
        assert_eq!(score.overall, 0.5);
        assert_eq!(score.duplication, 0.0);
        assert!(score.duplicate_of.is_none());
    }

    #[test]
    fn test_relevance_score_calculate() {
        let mut score = RelevanceScore::new("test query");
        score.semantic_similarity = 0.8;
        score.keyword_score = 0.6;
        score.task_relevance = 0.7;
        score.recency_boost = 0.9;
        score.calculate_combined();

        // (0.8 * 0.5) + (0.6 * 0.2) + (0.7 * 0.2) + (0.9 * 0.1)
        // = 0.4 + 0.12 + 0.14 + 0.09 = 0.75
        assert!((score.combined - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_extracted_entities_default() {
        let entities = ExtractedEntities::default();
        assert!(entities.people.is_empty());
        assert!(entities.dates.is_empty());
        assert!(entities.code_refs.is_empty());
        assert!(entities.urls.is_empty());
    }

    #[test]
    fn test_content_categorization_default() {
        let cat = ContentCategorization::default();
        assert_eq!(cat.topic, Topic::Other);
        assert_eq!(cat.sentiment, Sentiment::Neutral);
        assert_eq!(cat.priority, Priority::Medium);
    }

    #[test]
    fn test_topic_serialization() {
        let topic = Topic::BugReport;
        let json = serde_json::to_string(&topic).unwrap();
        assert_eq!(json, "\"bug_report\"");
    }

    #[test]
    fn test_sentiment_serialization() {
        let sentiment = Sentiment::Positive;
        let json = serde_json::to_string(&sentiment).unwrap();
        assert_eq!(json, "\"positive\"");
    }
}

/// Main heuristic analyzer that orchestrates all analysis
pub struct HeuristicAnalyzer;

impl HeuristicAnalyzer {
    /// Perform comprehensive heuristic analysis on content
    pub fn analyze(
        content_item_id: Uuid,
        text: &str,
        content_type: &str,
        source_type: &str,
        modified_at: Option<DateTime<Utc>>,
    ) -> HeuristicAnalysis {
        // Extract entities
        let entities = EntityExtractor::extract(text);

        // Categorize content
        let categorization = ContentCategorizer::categorize(text, content_type);

        // Calculate quality metrics
        let quality = QualityAnalyzer::analyze(text, source_type, modified_at);

        HeuristicAnalysis {
            content_item_id,
            entities,
            categorization,
            quality,
            relevance: None, // Relevance is calculated separately with a query
            analyzed_at: Utc::now(),
        }
    }

    /// Perform analysis with relevance scoring
    pub fn analyze_with_query(
        content_item_id: Uuid,
        text: &str,
        content_type: &str,
        source_type: &str,
        modified_at: Option<DateTime<Utc>>,
        query: &str,
        semantic_similarity: f32,
    ) -> HeuristicAnalysis {
        // Perform base analysis
        let mut analysis = Self::analyze(
            content_item_id,
            text,
            content_type,
            source_type,
            modified_at,
        );

        // Calculate relevance
        let relevance = RelevanceScorer::score(
            query,
            text,
            semantic_similarity,
            &analysis.quality,
            analysis.categorization.topic,
        );

        analysis.relevance = Some(relevance);
        analysis
    }
}

#[cfg(test)]
mod analyzer_tests {
    use super::*;

    #[test]
    fn test_analyze_basic() {
        let id = Uuid::new_v4();
        let text = "TODO: Fix the bug in main.rs:123. Contact alice@example.com for details.";

        let analysis = HeuristicAnalyzer::analyze(id, text, "issue", "github", Some(Utc::now()));

        assert_eq!(analysis.content_item_id, id);
        assert!(!analysis.entities.people.is_empty());
        assert!(!analysis.entities.code_refs.is_empty());
        assert!(analysis.categorization.actionability.score > 0.0);
        assert!(analysis.quality.overall > 0.0);
        assert!(analysis.relevance.is_none());
    }

    #[test]
    fn test_analyze_with_query() {
        let id = Uuid::new_v4();
        let text = "Here is how to implement user authentication using JWT tokens.";
        let query = "implement authentication";

        let analysis = HeuristicAnalyzer::analyze_with_query(
            id,
            text,
            "documentation",
            "markdown",
            Some(Utc::now()),
            query,
            0.85,
        );

        assert_eq!(analysis.content_item_id, id);
        assert!(analysis.relevance.is_some());

        let relevance = analysis.relevance.unwrap();
        assert_eq!(relevance.query, query);
        assert_eq!(relevance.semantic_similarity, 0.85);
        assert!(relevance.keyword_score > 0.0);
        assert!(relevance.combined > 0.0);
    }

    #[test]
    fn test_analyze_empty_text() {
        let id = Uuid::new_v4();
        let analysis = HeuristicAnalyzer::analyze(id, "", "text", "unknown", None);

        assert_eq!(analysis.content_item_id, id);
        assert!(analysis.entities.people.is_empty());
        assert!(analysis.entities.urls.is_empty());
        assert!(analysis.quality.density == 0.0);
    }

    #[test]
    fn test_analyze_comprehensive() {
        let id = Uuid::new_v4();
        let text = r#"
            # Bug Report: Authentication Error

            Contact: john@example.com
            File: src/auth/login.rs:45
            URL: https://github.com/project/repo/issues/123

            TODO: Fix the critical authentication bug
            Deadline: 2024-12-31

            The `authenticate_user` function fails when...

            #urgent #security @reviewer
        "#;

        let analysis =
            HeuristicAnalyzer::analyze(id, text, "markdown", "github_issue", Some(Utc::now()));

        // Should extract emails
        assert!(!analysis.entities.people.is_empty());
        assert!(analysis.entities.people.iter().any(|p| p.email.is_some()));

        // Should extract URLs
        assert!(!analysis.entities.urls.is_empty());

        // Should extract dates
        assert!(!analysis.entities.dates.is_empty());

        // Should extract code references
        assert!(!analysis.entities.code_refs.is_empty());

        // Should detect bug report topic
        assert_eq!(analysis.categorization.topic, Topic::BugReport);

        // Should detect critical priority
        assert_eq!(analysis.categorization.priority, Priority::Critical);

        // Should detect actionability
        assert!(analysis.categorization.actionability.score > 0.0);

        // Should extract tags
        assert!(!analysis.categorization.tags.is_empty());

        // Should have quality metrics
        assert!(analysis.quality.overall > 0.0);
        assert!(analysis.quality.freshness > 0.0);
    }
}

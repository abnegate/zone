//! Relevance scoring for content
//!
//! Scores content relevance to a query using:
//! - Semantic similarity (from embeddings)
//! - Keyword overlap
//! - Task relevance (topic matching)
//! - Recency boost

use super::{QualityScore, RelevanceScore, Topic};
use std::collections::HashSet;

/// Relevance scorer
pub struct RelevanceScorer;

impl RelevanceScorer {
    /// Calculate comprehensive relevance score
    pub fn score(
        query: &str,
        text: &str,
        semantic_similarity: f32,
        quality: &QualityScore,
        topic: Topic,
    ) -> RelevanceScore {
        let keyword_score = Self::calculate_keyword_score(query, text);
        let task_relevance = Self::calculate_task_relevance(query, topic);
        let recency_boost = Self::calculate_recency_boost(quality.freshness);

        let mut score = RelevanceScore::new(query);
        score.semantic_similarity = semantic_similarity;
        score.keyword_score = keyword_score;
        score.task_relevance = task_relevance;
        score.recency_boost = recency_boost;
        score.calculate_combined();

        score
    }

    /// Calculate keyword overlap score using Jaccard similarity
    pub fn calculate_keyword_score(query: &str, text: &str) -> f32 {
        if query.is_empty() || text.is_empty() {
            return 0.0;
        }

        // Normalize and tokenize
        let query_words: HashSet<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2) // Ignore very short words
            .map(|w| w.to_string())
            .collect();

        let text_words: HashSet<String> = text
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_string())
            .collect();

        if query_words.is_empty() || text_words.is_empty() {
            return 0.0;
        }

        // Calculate Jaccard similarity: |intersection| / |union|
        let intersection_count = query_words.intersection(&text_words).count();
        let union_count = query_words.len() + text_words.len() - intersection_count;

        let jaccard = intersection_count as f32 / union_count as f32;

        // Also calculate overlap ratio (matches / query_words)
        let overlap = intersection_count as f32 / query_words.len() as f32;

        // Use weighted combination, favoring overlap ratio
        (jaccard * 0.3 + overlap * 0.7).min(1.0)
    }

    /// Calculate task relevance based on query intent and topic
    pub fn calculate_task_relevance(query: &str, topic: Topic) -> f32 {
        let lower = query.to_lowercase();

        // Detect query intent
        let is_code_query = lower.contains("function")
            || lower.contains("implement")
            || lower.contains("code")
            || lower.contains("class")
            || lower.contains("method");

        let is_doc_query = lower.contains("how to")
            || lower.contains("documentation")
            || lower.contains("guide")
            || lower.contains("example")
            || lower.contains("tutorial");

        let is_bug_query = lower.contains("bug")
            || lower.contains("error")
            || lower.contains("fix")
            || lower.contains("broken")
            || lower.contains("issue");

        let is_planning_query = lower.contains("roadmap")
            || lower.contains("plan")
            || lower.contains("timeline")
            || lower.contains("schedule");

        let is_review_query =
            lower.contains("review") || lower.contains("feedback") || lower.contains("opinion");

        // Match intent to topic
        match topic {
            Topic::Code => {
                if is_code_query {
                    0.9
                } else if is_doc_query {
                    0.6
                } else {
                    0.3
                }
            }
            Topic::Documentation => {
                if is_doc_query {
                    0.9
                } else if is_code_query {
                    0.5
                } else {
                    0.4
                }
            }
            Topic::BugReport => {
                if is_bug_query {
                    0.9
                } else if is_code_query {
                    0.6
                } else {
                    0.3
                }
            }
            Topic::Planning => {
                if is_planning_query {
                    0.9
                } else {
                    0.4
                }
            }
            Topic::Review => {
                if is_review_query {
                    0.9
                } else if is_code_query {
                    0.6
                } else {
                    0.4
                }
            }
            Topic::FeatureRequest => {
                if is_planning_query || is_code_query {
                    0.7
                } else {
                    0.4
                }
            }
            Topic::Question | Topic::Answer
                if lower.contains('?') || lower.contains("how") || lower.contains("what") =>
            {
                0.8
            }
            Topic::Question | Topic::Answer => 0.5,
            _ => 0.5, // Default medium relevance
        }
    }

    /// Calculate recency boost from freshness score
    pub fn calculate_recency_boost(freshness: f32) -> f32 {
        // Recency boost is non-linear: fresh content gets higher boost
        if freshness > 0.9 {
            1.0 // Very fresh
        } else if freshness > 0.7 {
            0.8 // Fresh
        } else if freshness > 0.5 {
            0.6 // Moderately fresh
        } else if freshness > 0.3 {
            0.4 // Somewhat stale
        } else {
            0.2 // Stale
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Keyword score tests
    #[test]
    fn test_keyword_score_exact_match() {
        let query = "implement user authentication";
        let text = "Here is how to implement user authentication in Rust";

        let score = RelevanceScorer::calculate_keyword_score(query, text);

        // Should have very high score (all query words present)
        assert!(score > 0.7);
        println!("Exact match score: {}", score);
    }

    #[test]
    fn test_keyword_score_partial_match() {
        let query = "implement user authentication database";
        let text = "This function handles user authentication";

        let score = RelevanceScorer::calculate_keyword_score(query, text);

        // Should have medium score (some query words present)
        assert!(score > 0.3 && score < 0.8);
        println!("Partial match score: {}", score);
    }

    #[test]
    fn test_keyword_score_no_match() {
        let query = "database connection";
        let text = "Frontend styling and layout";

        let score = RelevanceScorer::calculate_keyword_score(query, text);

        // Should have very low score
        assert!(score < 0.2);
        println!("No match score: {}", score);
    }

    #[test]
    fn test_keyword_score_empty_query() {
        let score = RelevanceScorer::calculate_keyword_score("", "some text");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_keyword_score_empty_text() {
        let score = RelevanceScorer::calculate_keyword_score("query", "");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_keyword_score_case_insensitive() {
        let query = "IMPLEMENT USER";
        let text = "implement user authentication";

        let score = RelevanceScorer::calculate_keyword_score(query, text);

        // Should match despite case difference
        assert!(score > 0.7);
    }

    // Task relevance tests
    #[test]
    fn test_task_relevance_code_query() {
        let query = "implement a function to calculate total";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::Code);

        assert!(score > 0.8);
        println!("Code query to Code topic: {}", score);
    }

    #[test]
    fn test_task_relevance_doc_query() {
        let query = "how to use the API documentation";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::Documentation);

        assert!(score > 0.8);
        println!("Doc query to Doc topic: {}", score);
    }

    #[test]
    fn test_task_relevance_bug_query() {
        let query = "fix the error in authentication";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::BugReport);

        assert!(score > 0.8);
        println!("Bug query to Bug topic: {}", score);
    }

    #[test]
    fn test_task_relevance_mismatch() {
        let query = "implement a function";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::BugReport);

        // Code query to bug topic should have lower relevance
        assert!(score < 0.7);
        println!("Code query to Bug topic: {}", score);
    }

    #[test]
    fn test_task_relevance_planning() {
        let query = "show me the roadmap for next quarter";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::Planning);

        assert!(score > 0.8);
    }

    #[test]
    fn test_task_relevance_review() {
        let query = "need feedback on this implementation";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::Review);

        assert!(score > 0.8);
    }

    #[test]
    fn test_task_relevance_question() {
        let query = "how does this work?";
        let score = RelevanceScorer::calculate_task_relevance(query, Topic::Question);

        assert!(score > 0.7);
    }

    // Recency boost tests
    #[test]
    fn test_recency_boost_fresh() {
        let boost = RelevanceScorer::calculate_recency_boost(0.95);
        assert_eq!(boost, 1.0);
    }

    #[test]
    fn test_recency_boost_somewhat_fresh() {
        let boost = RelevanceScorer::calculate_recency_boost(0.75);
        assert_eq!(boost, 0.8);
    }

    #[test]
    fn test_recency_boost_medium() {
        let boost = RelevanceScorer::calculate_recency_boost(0.55);
        assert_eq!(boost, 0.6);
    }

    #[test]
    fn test_recency_boost_stale() {
        let boost = RelevanceScorer::calculate_recency_boost(0.2);
        assert_eq!(boost, 0.2);
    }

    #[test]
    fn test_recency_boost_very_stale() {
        let boost = RelevanceScorer::calculate_recency_boost(0.05);
        assert_eq!(boost, 0.2);
    }

    // Combined score tests
    #[test]
    fn test_combined_score_calculation() {
        let query = "implement authentication function";
        let text = "Here is how to implement the authentication function in our codebase";

        let quality = QualityScore {
            overall: 0.8,
            freshness: 0.9,
            reliability: 0.8,
            density: 0.7,
            duplication: 0.0,
            duplicate_of: None,
        };

        let score = RelevanceScorer::score(
            query,
            text,
            0.85, // High semantic similarity
            &quality,
            Topic::Code,
        );

        // Should have high combined score
        assert!(score.combined > 0.7);
        assert!(score.keyword_score > 0.6);
        assert!(score.task_relevance > 0.8);
        assert_eq!(score.recency_boost, 0.8);

        println!("Combined score: {}", score.combined);
        println!("  Semantic: {}", score.semantic_similarity);
        println!("  Keyword: {}", score.keyword_score);
        println!("  Task: {}", score.task_relevance);
        println!("  Recency: {}", score.recency_boost);
    }

    #[test]
    fn test_combined_score_low_relevance() {
        let query = "database schema design";
        let text = "Frontend component styling and layout best practices";

        let quality = QualityScore {
            overall: 0.5,
            freshness: 0.3,
            reliability: 0.5,
            density: 0.6,
            duplication: 0.0,
            duplicate_of: None,
        };

        let score = RelevanceScorer::score(
            query,
            text,
            0.2, // Low semantic similarity
            &quality,
            Topic::Documentation,
        );

        // Should have low combined score
        assert!(score.combined < 0.4);
        println!("Low relevance combined score: {}", score.combined);
    }

    #[test]
    fn test_combined_score_medium() {
        let query = "error handling patterns";
        let text = "Common patterns for handling errors in production code";

        let quality = QualityScore {
            overall: 0.6,
            freshness: 0.5,
            reliability: 0.7,
            density: 0.6,
            duplication: 0.0,
            duplicate_of: None,
        };

        let score = RelevanceScorer::score(
            query,
            text,
            0.6, // Medium semantic similarity
            &quality,
            Topic::Code,
        );

        assert!(score.combined > 0.4 && score.combined < 0.8);
        println!("Medium relevance combined score: {}", score.combined);
    }
}

//! Quality metrics for content
//!
//! Calculates quality scores based on:
//! - Freshness: exponential decay based on age
//! - Reliability: source type credibility
//! - Density: information density (unique words ratio)
//! - Duplication: content similarity (placeholder)

use super::QualityScore;
use chrono::{DateTime, Utc};
use std::collections::HashSet;

/// Quality analyzer for content
pub struct QualityAnalyzer;

impl QualityAnalyzer {
    /// Analyze overall quality of content
    pub fn analyze(
        text: &str,
        source_type: &str,
        modified_at: Option<DateTime<Utc>>,
    ) -> QualityScore {
        let freshness = Self::calculate_freshness(modified_at);
        let reliability = Self::calculate_reliability(source_type);
        let density = Self::calculate_density(text);
        let duplication = Self::calculate_duplication(text, ""); // Placeholder hash

        // Overall score is weighted average
        let overall = (freshness * 0.25)
            + (reliability * 0.35)
            + (density * 0.30)
            + ((1.0 - duplication) * 0.10);

        QualityScore {
            overall,
            freshness,
            reliability,
            density,
            duplication,
            duplicate_of: None,
        }
    }

    /// Calculate freshness score based on age
    /// 1.0 = today, ~0.5 after 7 days, ~0.1 after 30 days
    pub fn calculate_freshness(modified_at: Option<DateTime<Utc>>) -> f32 {
        match modified_at {
            None => 0.5, // Unknown age gets medium score
            Some(dt) => {
                let now = Utc::now();
                let age_seconds = (now - dt).num_seconds().max(0);

                // Handle very old content explicitly (over a year old)
                if age_seconds > 86400 * 365 {
                    return 0.01; // Minimum freshness
                }

                // Exponential decay with half-life of ~7 days (604800 seconds)
                // Formula: e^(-age / half_life)
                let half_life = 604800.0; // 7 days in seconds
                let decay = (-(age_seconds as f64) / half_life).exp() as f32;

                decay.clamp(0.01, 1.0) // Clamp between 0.01 and 1.0
            }
        }
    }

    /// Calculate reliability score based on source type
    pub fn calculate_reliability(source_type: &str) -> f32 {
        let lower = source_type.to_lowercase();

        // Check more specific patterns first
        if lower.contains("comment") || lower.contains("review") {
            0.6 // Comments/reviews are moderately reliable
        } else if lower.contains("doc")
            || lower.contains("readme")
            || lower.contains("config")
            || lower.contains("settings")
        {
            0.9 // Documentation and configuration files are highly reliable
        } else if lower.contains("code") || lower.contains("src") {
            0.8 // Source code is quite reliable
        } else if lower.contains("test") {
            0.75 // Tests are fairly reliable
        } else if lower.contains("chat") || lower.contains("message") {
            0.4 // Chat messages are less reliable
        } else if lower.contains("draft") || lower.contains("temp") {
            0.3 // Drafts/temp content is low reliability
        } else {
            0.5 // Default medium reliability
        }
    }

    /// Calculate information density
    /// Ratio of unique words to total words
    pub fn calculate_density(text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }

        let words: Vec<&str> = text
            .split_whitespace()
            .filter(|w| w.len() > 2) // Filter out very short words
            .collect();

        if words.is_empty() {
            return 0.0;
        }

        let total_words = words.len();
        let unique_words: HashSet<String> = words.into_iter().map(|w| w.to_lowercase()).collect();
        let unique_count = unique_words.len();

        // Calculate ratio

        // Return raw ratio - it's a reasonable metric as-is
        // High ratio (close to 1.0) = high diversity
        // Low ratio (close to 0) = lots of repetition
        unique_count as f32 / total_words as f32
    }

    /// Calculate duplication score
    /// 0.0 = unique, 1.0 = exact duplicate
    /// Placeholder implementation - would use content hashing in production
    pub fn calculate_duplication(_text: &str, _hash: &str) -> f32 {
        // TODO: Implement actual content hash comparison
        // For now, always return 0.0 (unique)
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // Freshness tests
    #[test]
    fn test_freshness_today() {
        let now = Utc::now();
        let freshness = QualityAnalyzer::calculate_freshness(Some(now));

        // Should be very close to 1.0
        assert!(freshness > 0.99);
        assert!(freshness <= 1.0);
    }

    #[test]
    fn test_freshness_week_ago() {
        let week_ago = Utc::now() - Duration::days(7);
        let freshness = QualityAnalyzer::calculate_freshness(Some(week_ago));

        // Should be around 0.5 (half-life is 7 days)
        assert!(freshness > 0.3 && freshness < 0.7);
        println!("Week ago freshness: {}", freshness);
    }

    #[test]
    fn test_freshness_month_ago() {
        let month_ago = Utc::now() - Duration::days(30);
        let freshness = QualityAnalyzer::calculate_freshness(Some(month_ago));

        // Should be low, around 0.1 or less
        assert!(freshness < 0.2);
        println!("Month ago freshness: {}", freshness);
    }

    #[test]
    fn test_freshness_none() {
        let freshness = QualityAnalyzer::calculate_freshness(None);

        // Should return default medium score
        assert_eq!(freshness, 0.5);
    }

    #[test]
    fn test_freshness_hour_ago() {
        let hour_ago = Utc::now() - Duration::hours(1);
        let freshness = QualityAnalyzer::calculate_freshness(Some(hour_ago));

        // Should be very high, close to 1.0
        assert!(freshness > 0.95);
    }

    // Reliability tests
    #[test]
    fn test_reliability_documentation() {
        let score = QualityAnalyzer::calculate_reliability("documentation");
        assert_eq!(score, 0.9);

        let score2 = QualityAnalyzer::calculate_reliability("README.md");
        assert_eq!(score2, 0.9);
    }

    #[test]
    fn test_reliability_code() {
        let score = QualityAnalyzer::calculate_reliability("source_code");
        assert_eq!(score, 0.8);

        let score2 = QualityAnalyzer::calculate_reliability("src/main.rs");
        assert_eq!(score2, 0.8);
    }

    #[test]
    fn test_reliability_chat() {
        let score = QualityAnalyzer::calculate_reliability("chat_message");
        assert_eq!(score, 0.4);
    }

    #[test]
    fn test_reliability_comment() {
        let score = QualityAnalyzer::calculate_reliability("code_comment");
        assert_eq!(score, 0.6);
    }

    #[test]
    fn test_reliability_config() {
        let score = QualityAnalyzer::calculate_reliability("config.yaml");
        assert_eq!(score, 0.9);
    }

    #[test]
    fn test_reliability_test() {
        let score = QualityAnalyzer::calculate_reliability("test_file");
        assert_eq!(score, 0.75);
    }

    #[test]
    fn test_reliability_default() {
        let score = QualityAnalyzer::calculate_reliability("unknown");
        assert_eq!(score, 0.5);
    }

    // Density tests
    #[test]
    fn test_density_high() {
        // Most words are unique
        let text = "implement feature using rust language backend framework integration";
        let density = QualityAnalyzer::calculate_density(text);

        assert!(density > 0.8);
        println!("High density: {}", density);
    }

    #[test]
    fn test_density_low() {
        // Many repeated words
        let text = "the the the the code code code is is is good good good";
        let density = QualityAnalyzer::calculate_density(text);

        assert!(density < 0.5);
        println!("Low density: {}", density);
    }

    #[test]
    fn test_density_empty() {
        let density = QualityAnalyzer::calculate_density("");
        assert_eq!(density, 0.0);
    }

    #[test]
    fn test_density_medium() {
        let text = "This is a test sentence with some repeated words. \
                    This test checks the density of words in a sentence.";
        let density = QualityAnalyzer::calculate_density(text);

        assert!(density > 0.3 && density < 0.9);
        println!("Medium density: {}", density);
    }

    #[test]
    fn test_density_very_sparse() {
        // Every word unique but very short text
        let text = "abc def ghi jkl mno";
        let density = QualityAnalyzer::calculate_density(text);

        assert!(density > 0.5);
    }

    // Duplication tests
    #[test]
    fn test_duplication_placeholder() {
        // Current implementation always returns 0.0
        let dup = QualityAnalyzer::calculate_duplication("some text", "hash123");
        assert_eq!(dup, 0.0);
    }

    // Overall quality tests
    #[test]
    fn test_overall_quality_calculation() {
        let text = "High quality documentation with diverse vocabulary and clear explanations.";
        let now = Utc::now();

        let quality = QualityAnalyzer::analyze(text, "documentation", Some(now));

        // Should have high overall score due to:
        // - Fresh content (now)
        // - High reliability (documentation)
        // - Good density
        assert!(quality.overall > 0.7);
        assert!(quality.freshness > 0.9);
        assert_eq!(quality.reliability, 0.9);
        assert!(quality.density > 0.0);
        assert_eq!(quality.duplication, 0.0);
    }

    #[test]
    fn test_overall_quality_low() {
        let text = "the the the the the";
        let old = Utc::now() - Duration::days(60);

        let quality = QualityAnalyzer::analyze(text, "chat", Some(old));

        // Should have low overall score due to:
        // - Old content
        // - Low reliability (chat)
        // - Low density
        assert!(quality.overall < 0.5);
        assert!(quality.freshness < 0.1);
        assert_eq!(quality.reliability, 0.4);
    }

    #[test]
    fn test_overall_quality_medium() {
        let text = "Some regular text with a mix of unique and repeated words. \
                    Contains information but not highly dense.";
        let week_ago = Utc::now() - Duration::days(7);

        let quality = QualityAnalyzer::analyze(text, "code_comment", Some(week_ago));

        assert!(quality.overall > 0.2 && quality.overall < 0.8);
        println!("Medium quality score: {}", quality.overall);
    }
}

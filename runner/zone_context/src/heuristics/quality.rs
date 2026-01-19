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
    ///
    /// Uses SimHash for near-duplicate detection:
    /// - Computes a 64-bit fingerprint of the text
    /// - Compares fingerprints using Hamming distance
    /// - Returns similarity score based on matching bits
    pub fn calculate_duplication(text: &str, existing_hash: &str) -> f32 {
        if text.is_empty() || existing_hash.is_empty() {
            return 0.0;
        }

        // Compute SimHash fingerprint of the text
        let new_hash = Self::simhash(text);

        // Parse existing hash (expecting hex string)
        let existing_fingerprint = match u64::from_str_radix(existing_hash, 16) {
            Ok(fp) => fp,
            Err(_) => return 0.0, // Invalid hash format, assume unique
        };

        // Calculate Hamming distance (number of differing bits)
        let hamming_distance = (new_hash ^ existing_fingerprint).count_ones();

        // Convert to similarity score (0.0 = very different, 1.0 = identical)
        // Threshold: < 3 bits different = likely duplicate (similarity > 0.95)
        let similarity = 1.0 - (hamming_distance as f32 / 64.0);

        similarity.clamp(0.0, 1.0)
    }

    /// Compute SimHash fingerprint for text
    ///
    /// SimHash algorithm:
    /// 1. Tokenize text into words
    /// 2. Hash each word to 64-bit value
    /// 3. For each hash bit, increment or decrement a counter
    /// 4. Final fingerprint: bit is 1 if counter > 0, else 0
    fn simhash(text: &str) -> u64 {
        let mut v = [0i32; 64];

        // Tokenize and process each word
        for word in text.split_whitespace() {
            let word_lower = word.to_lowercase();
            // Simple hash using Rust's default hasher
            let hash = Self::hash_word(&word_lower);

            // Update counters for each bit
            for (i, count) in v.iter_mut().enumerate() {
                if (hash & (1u64 << i)) != 0 {
                    *count += 1;
                } else {
                    *count -= 1;
                }
            }
        }

        // Generate final fingerprint
        let mut fingerprint = 0u64;
        for (i, &count) in v.iter().enumerate() {
            if count > 0 {
                fingerprint |= 1u64 << i;
            }
        }

        fingerprint
    }

    /// Simple hash function for a word (FNV-1a hash)
    fn hash_word(word: &str) -> u64 {
        const FNV_OFFSET: u64 = 14695981039346656037;
        const FNV_PRIME: u64 = 1099511628211;

        let mut hash = FNV_OFFSET;
        for byte in word.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
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
    fn test_duplication_identical() {
        let text = "The quick brown fox jumps over the lazy dog";
        let hash = QualityAnalyzer::simhash(text);
        let hash_str = format!("{:016x}", hash);

        let dup = QualityAnalyzer::calculate_duplication(text, &hash_str);
        assert_eq!(dup, 1.0); // Exact match
    }

    #[test]
    fn test_duplication_similar() {
        let text1 = "The quick brown fox jumps over the lazy dog";
        let text2 = "The quick brown fox jumps over a lazy dog"; // Very similar
        let hash1 = QualityAnalyzer::simhash(text1);
        let hash_str = format!("{:016x}", hash1);

        let dup = QualityAnalyzer::calculate_duplication(text2, &hash_str);
        assert!(dup > 0.8); // Should be very similar
    }

    #[test]
    fn test_duplication_different() {
        let text1 = "The quick brown fox jumps over the lazy dog";
        let text2 = "Rust is a systems programming language";
        let hash1 = QualityAnalyzer::simhash(text1);
        let hash_str = format!("{:016x}", hash1);

        let dup = QualityAnalyzer::calculate_duplication(text2, &hash_str);
        assert!(dup < 0.5); // Should be different
    }

    #[test]
    fn test_duplication_empty() {
        let dup = QualityAnalyzer::calculate_duplication("", "");
        assert_eq!(dup, 0.0);

        let dup2 = QualityAnalyzer::calculate_duplication("some text", "");
        assert_eq!(dup2, 0.0);
    }

    #[test]
    fn test_duplication_invalid_hash() {
        let dup = QualityAnalyzer::calculate_duplication("some text", "invalid_hex");
        assert_eq!(dup, 0.0);
    }

    #[test]
    fn test_simhash_consistency() {
        let text = "Hello world this is a test";
        let hash1 = QualityAnalyzer::simhash(text);
        let hash2 = QualityAnalyzer::simhash(text);
        assert_eq!(hash1, hash2); // Same text should produce same hash
    }

    #[test]
    fn test_simhash_different_texts() {
        let text1 = "Hello world";
        let text2 = "Goodbye world";
        let hash1 = QualityAnalyzer::simhash(text1);
        let hash2 = QualityAnalyzer::simhash(text2);
        assert_ne!(hash1, hash2); // Different texts should produce different hashes
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

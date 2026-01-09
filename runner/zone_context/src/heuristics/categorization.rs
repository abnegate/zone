//! Content categorization
//!
//! Categorizes content by:
//! - Topic (code, documentation, bug report, etc.)
//! - Sentiment (positive, negative, neutral, mixed)
//! - Priority (critical, high, medium, low)
//! - Actionability (tasks, questions, decisions)
//! - Tags (hashtags, keywords)

use super::{ActionType, ActionabilityScore, ContentCategorization, Priority, Sentiment, Topic};
use regex::Regex;
use std::sync::OnceLock;

/// Content categorizer using keyword-based detection
pub struct ContentCategorizer;

impl ContentCategorizer {
    /// Categorize content comprehensively
    pub fn categorize(text: &str, content_type: &str) -> ContentCategorization {
        let (topic, topic_confidence) = Self::detect_topic(text, content_type);
        let sentiment = Self::detect_sentiment(text);
        let priority = Self::detect_priority(text);
        let actionability = Self::detect_actionability(text);
        let tags = Self::extract_tags(text);

        ContentCategorization {
            topic,
            topic_confidence,
            sentiment,
            priority,
            actionability,
            tags,
        }
    }

    /// Detect primary topic from text and content type
    pub fn detect_topic(text: &str, content_type: &str) -> (Topic, f32) {
        let lower = text.to_lowercase();
        let mut scores: Vec<(Topic, f32)> = Vec::new();

        // Code patterns
        let code_keywords = [
            "fn ",
            "function",
            "class ",
            "def ",
            "impl ",
            "trait",
            "struct",
            "interface",
            "const ",
            "let ",
            "var ",
            "import",
            "export",
        ];
        let code_score = Self::keyword_density(&lower, &code_keywords);
        if code_score > 0.0 || content_type.contains("code") {
            scores.push((Topic::Code, code_score + 0.3));
        }

        // Documentation patterns
        let doc_keywords = [
            "documentation",
            "readme",
            "guide",
            "tutorial",
            "api reference",
            "how to",
            "example",
            "usage",
            "##",
            "###",
        ];
        let doc_score = Self::keyword_density(&lower, &doc_keywords);
        if doc_score > 0.0 || content_type.contains("doc") || content_type.contains("md") {
            scores.push((Topic::Documentation, doc_score + 0.3));
        }

        // Bug report patterns
        let bug_keywords = [
            "bug",
            "error",
            "crash",
            "broken",
            "failure",
            "failed",
            "exception",
            "stack trace",
            "reproduce",
            "regression",
            "fix",
            "bug report",
        ];
        let bug_score = Self::keyword_density(&lower, &bug_keywords);
        if bug_score > 0.0 {
            scores.push((Topic::BugReport, bug_score + 0.3));
        }

        // Feature request patterns
        let feature_keywords = [
            "feature request",
            "enhancement",
            "would be nice",
            "suggestion",
            "proposal",
            "new feature",
            "add support for",
        ];
        let feature_score = Self::keyword_density(&lower, &feature_keywords);
        if feature_score > 0.0 {
            scores.push((Topic::FeatureRequest, feature_score + 0.2));
        }

        // Question patterns
        let question_keywords = [
            "?", "how do", "how to", "why", "what", "when", "where", "help",
        ];
        let question_score = Self::keyword_density(&lower, &question_keywords);
        if question_score > 0.0 {
            scores.push((Topic::Question, question_score + 0.1));
        }

        // Review patterns
        let review_keywords = [
            "review",
            "lgtm",
            "looks good",
            "approve",
            "comment on",
            "code review",
            "pull request",
            "pr",
        ];
        let review_score = Self::keyword_density(&lower, &review_keywords);
        if review_score > 0.0 {
            scores.push((Topic::Review, review_score + 0.2));
        }

        // Planning patterns
        let planning_keywords = [
            "roadmap",
            "milestone",
            "sprint",
            "planning",
            "schedule",
            "timeline",
            "estimate",
            "epic",
        ];
        let planning_score = Self::keyword_density(&lower, &planning_keywords);
        if planning_score > 0.0 {
            scores.push((Topic::Planning, planning_score + 0.2));
        }

        // Meeting patterns
        let meeting_keywords = [
            "meeting",
            "agenda",
            "minutes",
            "standup",
            "sync",
            "discussion",
        ];
        let meeting_score = Self::keyword_density(&lower, &meeting_keywords);
        if meeting_score > 0.0 {
            scores.push((Topic::Meeting, meeting_score + 0.2));
        }

        // Announcement patterns
        let announcement_keywords = [
            "announcement",
            "release",
            "launched",
            "available now",
            "introducing",
            "pleased to announce",
        ];
        let announcement_score = Self::keyword_density(&lower, &announcement_keywords);
        if announcement_score > 0.0 {
            scores.push((Topic::Announcement, announcement_score + 0.2));
        }

        // Get highest scoring topic
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((topic, score)) = scores.first() {
            (*topic, score.min(1.0))
        } else {
            (Topic::Other, 0.1)
        }
    }

    /// Calculate keyword density (0.0-1.0)
    fn keyword_density(text: &str, keywords: &[&str]) -> f32 {
        let lower = text.to_lowercase();
        let total_words = lower.split_whitespace().count().max(1);
        let mut matches = 0;

        for keyword in keywords {
            matches += lower.matches(keyword).count();
        }

        (matches as f32 / total_words as f32).min(1.0)
    }

    /// Detect sentiment from text
    pub fn detect_sentiment(text: &str) -> Sentiment {
        let lower = text.to_lowercase();

        let positive_keywords = [
            "great",
            "excellent",
            "good",
            "thanks",
            "thank you",
            "works",
            "solved",
            "fixed",
            "perfect",
            "awesome",
            "love",
            "appreciate",
            "helpful",
            "success",
            "resolved",
        ];

        let negative_keywords = [
            "broken",
            "failed",
            "error",
            "issue",
            "problem",
            "bug",
            "crash",
            "doesn't work",
            "not working",
            "wrong",
            "bad",
            "terrible",
            "frustrating",
            "disappointed",
            "hate",
        ];

        let positive_count = positive_keywords
            .iter()
            .filter(|k| lower.contains(*k))
            .count();
        let negative_count = negative_keywords
            .iter()
            .filter(|k| lower.contains(*k))
            .count();

        if positive_count > 0 && negative_count > 0 {
            Sentiment::Mixed
        } else if positive_count > negative_count && positive_count > 0 {
            Sentiment::Positive
        } else if negative_count > positive_count && negative_count > 0 {
            Sentiment::Negative
        } else {
            Sentiment::Neutral
        }
    }

    /// Detect priority level
    pub fn detect_priority(text: &str) -> Priority {
        let lower = text.to_lowercase();

        let critical_keywords = [
            "critical",
            "urgent",
            "asap",
            "emergency",
            "blocker",
            "blocking",
            "production down",
            "outage",
            "immediately",
        ];

        let high_keywords = [
            "important",
            "priority",
            "high priority",
            "soon",
            "required",
            "must have",
        ];

        let low_keywords = [
            "nice to have",
            "low priority",
            "when possible",
            "eventually",
        ];

        // Check low priority first to avoid false positives
        if low_keywords.iter().any(|k| lower.contains(k)) {
            Priority::Low
        } else if critical_keywords.iter().any(|k| lower.contains(k)) {
            Priority::Critical
        } else if high_keywords.iter().any(|k| lower.contains(k)) {
            Priority::High
        } else {
            Priority::Medium
        }
    }

    /// Detect actionability and extract action items
    pub fn detect_actionability(text: &str) -> ActionabilityScore {
        let lower = text.to_lowercase();
        let mut action_items = Vec::new();
        let mut score: f32 = 0.0;
        let mut action_type = ActionType::Information;
        let mut requires_response = false;

        // TODO/FIXME patterns
        static TODO_RE: OnceLock<Regex> = OnceLock::new();
        let todo_re = TODO_RE.get_or_init(|| {
            Regex::new(r"(?i)\b(?:TODO|FIXME|HACK|XXX|NOTE):\s*([^\n]{1,500})").unwrap()
        });

        let mut todo_count = 0;
        for cap in todo_re.captures_iter(text) {
            if let Some(item) = cap.get(1) {
                action_items.push(item.as_str().trim().to_string());
                todo_count += 1;
                action_type = ActionType::Task;
            }
        }
        // Use logarithmic scaling for TODO items (capped at 0.4)
        let todo_score = (todo_count as f32 * 0.15).min(0.4);
        score += todo_score;

        // Task patterns (check first to prioritize over feedback/review)
        let task_keywords = [
            "please",
            "need to",
            "we should",
            "you should",
            "must",
            "action item",
        ];
        let has_task = task_keywords.iter().any(|k| lower.contains(k));
        if has_task {
            score += 0.4;
            action_type = ActionType::Task;
        }

        // Feedback patterns (check before generic questions)
        let feedback_keywords = [
            "feedback",
            "review",
            "thoughts",
            "opinion",
            "what do you think",
        ];
        let has_feedback = feedback_keywords.iter().any(|k| lower.contains(k));
        if has_feedback && !has_task {
            score += 0.4;
            action_type = ActionType::FeedbackRequest;
            requires_response = true;
        }

        // Question patterns (check after feedback to avoid overriding)
        if text.contains('?') || lower.contains("how do") || lower.contains("can you") {
            score += 0.3;
            if action_type == ActionType::Information {
                action_type = ActionType::Question;
                requires_response = true;
            }
        }

        // Decision patterns
        let decision_keywords = ["decide", "decision", "choice", "option", "approve"];
        if decision_keywords.iter().any(|k| lower.contains(k)) {
            score += 0.3;
            if action_type == ActionType::Information || action_type == ActionType::Task {
                action_type = ActionType::Decision;
                requires_response = true;
            }
        }

        // List-based action items (- [ ] or - tasks)
        static LIST_RE: OnceLock<Regex> = OnceLock::new();
        let list_re =
            LIST_RE.get_or_init(|| Regex::new(r"(?m)^[\s]*[-*]\s*\[[ x]\]\s*(.+)$").unwrap());

        let mut list_items = Vec::new();
        for cap in list_re.captures_iter(text) {
            if let Some(item) = cap.get(1) {
                list_items.push(item.as_str().trim().to_string());
                if action_type == ActionType::Information {
                    action_type = ActionType::Task;
                }
            }
        }
        // Use capped scaling for list items (capped at 0.2)
        let list_score = (list_items.len() as f32 * 0.05).min(0.2);
        score += list_score;
        action_items.extend(list_items);

        if score == 0.0 {
            action_type = ActionType::None;
        }

        ActionabilityScore {
            score: score.min(1.0),
            action_type,
            action_items,
            requires_response,
        }
    }

    /// Extract tags from text (hashtags and keywords)
    pub fn extract_tags(text: &str) -> Vec<String> {
        let mut tags = Vec::new();

        // Hashtags
        static HASHTAG_RE: OnceLock<Regex> = OnceLock::new();
        let hashtag_re = HASHTAG_RE.get_or_init(|| Regex::new(r"#([a-zA-Z0-9_]+)").unwrap());

        for cap in hashtag_re.captures_iter(text) {
            if let Some(tag) = cap.get(1) {
                tags.push(tag.as_str().to_lowercase());
            }
        }

        // @mentions
        static MENTION_RE: OnceLock<Regex> = OnceLock::new();
        let mention_re = MENTION_RE.get_or_init(|| Regex::new(r"@([a-zA-Z0-9_-]+)").unwrap());

        for cap in mention_re.captures_iter(text) {
            if let Some(mention) = cap.get(1) {
                tags.push(format!("@{}", mention.as_str()));
            }
        }

        // Deduplicate
        tags.sort();
        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Topic detection tests
    #[test]
    fn test_detect_topic_code() {
        let text = "The function calculate_total is defined with fn calculate_total() {}";
        let (topic, confidence) = ContentCategorizer::detect_topic(text, "rust");

        assert_eq!(topic, Topic::Code);
        assert!(confidence > 0.0);
    }

    #[test]
    fn test_detect_topic_documentation() {
        let text = "This is a guide on how to use the API. See the documentation for examples.";
        let (topic, confidence) = ContentCategorizer::detect_topic(text, "markdown");

        assert_eq!(topic, Topic::Documentation);
        assert!(confidence > 0.0);
    }

    #[test]
    fn test_detect_topic_bug_report() {
        let text = "Found a critical bug that causes a crash. The error occurs when...";
        let (topic, _) = ContentCategorizer::detect_topic(text, "issue");

        assert_eq!(topic, Topic::BugReport);
    }

    #[test]
    fn test_detect_topic_feature_request() {
        let text = "Feature request: would be nice to add support for dark mode";
        let (topic, _) = ContentCategorizer::detect_topic(text, "issue");

        assert_eq!(topic, Topic::FeatureRequest);
    }

    #[test]
    fn test_detect_topic_question() {
        let text = "How do I configure the database connection? What settings should I use?";
        let (topic, _) = ContentCategorizer::detect_topic(text, "chat");

        assert_eq!(topic, Topic::Question);
    }

    #[test]
    fn test_detect_topic_review() {
        let text = "Code review looks good. LGTM! Please approve this pull request.";
        let (topic, _) = ContentCategorizer::detect_topic(text, "comment");

        assert_eq!(topic, Topic::Review);
    }

    // Sentiment detection tests
    #[test]
    fn test_detect_sentiment_positive() {
        let text = "This is great! Thanks for the excellent work. Everything works perfectly.";
        let sentiment = ContentCategorizer::detect_sentiment(text);

        assert_eq!(sentiment, Sentiment::Positive);
    }

    #[test]
    fn test_detect_sentiment_negative() {
        let text = "This is broken and doesn't work. The error is terrible and frustrating.";
        let sentiment = ContentCategorizer::detect_sentiment(text);

        assert_eq!(sentiment, Sentiment::Negative);
    }

    #[test]
    fn test_detect_sentiment_neutral() {
        let text = "Here is the implementation of the feature.";
        let sentiment = ContentCategorizer::detect_sentiment(text);

        assert_eq!(sentiment, Sentiment::Neutral);
    }

    #[test]
    fn test_detect_sentiment_mixed() {
        let text = "Great idea but the implementation has bugs and doesn't work correctly.";
        let sentiment = ContentCategorizer::detect_sentiment(text);

        assert_eq!(sentiment, Sentiment::Mixed);
    }

    // Priority detection tests
    #[test]
    fn test_detect_priority_critical() {
        let text = "CRITICAL: Production is down! This is a blocker and needs immediate attention.";
        let priority = ContentCategorizer::detect_priority(text);

        assert_eq!(priority, Priority::Critical);
    }

    #[test]
    fn test_detect_priority_high() {
        let text = "This is important and high priority. We need this soon.";
        let priority = ContentCategorizer::detect_priority(text);

        assert_eq!(priority, Priority::High);
    }

    #[test]
    fn test_detect_priority_low() {
        let text = "This is nice to have when possible, low priority.";
        let priority = ContentCategorizer::detect_priority(text);

        assert_eq!(priority, Priority::Low);
    }

    #[test]
    fn test_detect_priority_medium() {
        let text = "We should implement this feature at some point.";
        let priority = ContentCategorizer::detect_priority(text);

        assert_eq!(priority, Priority::Medium);
    }

    // Actionability tests
    #[test]
    fn test_detect_actionability_todo() {
        let text = "TODO: Implement error handling\nFIXME: Fix the memory leak";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_type, ActionType::Task);
        assert_eq!(action.action_items.len(), 2);
        assert!(action.action_items[0].contains("Implement error handling"));
    }

    #[test]
    fn test_detect_actionability_question() {
        let text = "How do we handle this case? What should the behavior be?";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_type, ActionType::Question);
        assert!(action.requires_response);
    }

    #[test]
    fn test_detect_actionability_task() {
        let text = "We need to update the documentation. Please review this change.";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_type, ActionType::Task);
    }

    #[test]
    fn test_detect_actionability_decision() {
        let text = "We need to decide between option A and option B. Please approve.";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_type, ActionType::Decision);
        assert!(action.requires_response);
    }

    #[test]
    fn test_detect_actionability_feedback() {
        let text = "Looking for feedback on this approach. What do you think?";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_type, ActionType::FeedbackRequest);
        assert!(action.requires_response);
    }

    #[test]
    fn test_detect_actionability_checklist() {
        let text = "- [ ] Write tests\n- [x] Implement feature\n- [ ] Update docs";
        let action = ContentCategorizer::detect_actionability(text);

        assert!(action.score > 0.0);
        assert_eq!(action.action_items.len(), 3);
    }

    #[test]
    fn test_detect_actionability_none() {
        let text = "This is just informational content.";
        let action = ContentCategorizer::detect_actionability(text);

        assert_eq!(action.action_type, ActionType::None);
        assert!(!action.requires_response);
    }

    // Tag extraction tests
    #[test]
    fn test_extract_tags_hashtags() {
        let text = "Working on #rust #performance improvements";
        let tags = ContentCategorizer::extract_tags(text);

        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"performance".to_string()));
    }

    #[test]
    fn test_extract_tags_mentions() {
        let text = "CC @alice @bob for review";
        let tags = ContentCategorizer::extract_tags(text);

        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&"@alice".to_string()));
        assert!(tags.contains(&"@bob".to_string()));
    }

    #[test]
    fn test_extract_tags_mixed() {
        let text = "Issue #bugfix needs review by @reviewer #urgent";
        let tags = ContentCategorizer::extract_tags(text);

        assert!(tags.len() >= 2);
        assert!(tags.contains(&"bugfix".to_string()) || tags.contains(&"urgent".to_string()));
    }

    #[test]
    fn test_extract_tags_none() {
        let text = "Plain text without tags";
        let tags = ContentCategorizer::extract_tags(text);

        assert!(tags.is_empty());
    }

    #[test]
    fn test_categorize_full() {
        let text = "TODO: Fix the critical bug in main.rs. This is urgent! #bugfix";
        let cat = ContentCategorizer::categorize(text, "issue");

        assert_eq!(cat.priority, Priority::Critical);
        assert!(cat.actionability.score > 0.0);
        assert!(!cat.tags.is_empty());
    }
}

//! Entity extraction from text
//!
//! Extracts structured entities from unstructured text including:
//! - Email addresses
//! - URLs (classified by type)
//! - Dates (ISO, natural language, relative)
//! - Code references (functions, files with line numbers)
//! - File paths

use super::{
    CodeRefKind, CodeReference, DateEntity, ExtractedEntities, PersonEntity, UrlEntity, UrlType,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use std::sync::OnceLock;

/// Entity extractor using regex patterns
pub struct EntityExtractor;

impl EntityExtractor {
    /// Extract all entities from text
    pub fn extract(text: &str) -> ExtractedEntities {
        ExtractedEntities {
            people: Self::extract_emails(text),
            urls: Self::extract_urls(text),
            dates: Self::extract_dates(text),
            code_refs: Self::extract_code_refs(text),
            file_paths: Self::extract_file_paths(text),
            relationships: Vec::new(), // Not implemented yet
        }
    }

    /// Extract email addresses as PersonEntity instances
    pub fn extract_emails(text: &str) -> Vec<PersonEntity> {
        static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
        let re = EMAIL_RE.get_or_init(|| {
            Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap()
        });

        re.find_iter(text)
            .map(|m| {
                let email = m.as_str().to_string();
                // Extract name from email (before @)
                let name = email.split('@').next().unwrap_or(&email).to_string();
                PersonEntity {
                    name,
                    email: Some(email),
                    username: None,
                    role: None,
                }
            })
            .collect()
    }

    /// Extract and classify URLs
    pub fn extract_urls(text: &str) -> Vec<UrlEntity> {
        static URL_RE: OnceLock<Regex> = OnceLock::new();
        let re = URL_RE.get_or_init(|| {
            Regex::new(r#"https?://[^\s<>"']{1,2000}|www\.[^\s<>"']{1,2000}"#).unwrap()
        });

        re.find_iter(text)
            .map(|m| {
                let url = m.as_str().to_string();
                let url_type = Self::classify_url(&url);
                UrlEntity {
                    url,
                    url_type,
                    title: None,
                }
            })
            .collect()
    }

    /// Classify URL by type
    fn classify_url(url: &str) -> UrlType {
        let lower = url.to_lowercase();
        if lower.contains("github") && lower.contains("com") {
            UrlType::GitHub
        } else if lower.contains("gitlab") && lower.contains("com") {
            UrlType::GitLab
        } else if lower.contains("jira") || lower.contains("atlassian") {
            UrlType::Jira
        } else if lower.contains("confluence") {
            UrlType::Confluence
        } else if lower.contains("slack") && lower.contains("com") {
            UrlType::Slack
        } else if lower.contains("/api/") || lower.contains("/v1/") || lower.contains("/v2/") {
            UrlType::Api
        } else if lower.contains("docs") || lower.contains("documentation") || lower.ends_with("md")
        {
            UrlType::Documentation
        } else {
            UrlType::Web
        }
    }

    /// Extract date references
    pub fn extract_dates(text: &str) -> Vec<DateEntity> {
        let mut dates = Vec::new();

        // ISO 8601 dates (YYYY-MM-DD)
        static ISO_RE: OnceLock<Regex> = OnceLock::new();
        let iso_re = ISO_RE.get_or_init(|| {
            Regex::new(
                r"\b\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?)?\b",
            )
            .unwrap()
        });

        for m in iso_re.find_iter(text) {
            let raw = m.as_str().to_string();
            let parsed = if raw.contains('T') {
                DateTime::parse_from_rfc3339(&raw)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            } else {
                // Simple date without time
                format!("{}T00:00:00Z", raw).parse::<DateTime<Utc>>().ok()
            };

            dates.push(DateEntity {
                raw: raw.clone(),
                parsed,
                is_relative: false,
                is_deadline: Self::is_deadline_context(&raw, text),
            });
        }

        // Natural language dates (Jan 5, 2024 or January 5, 2024)
        static NATURAL_RE: OnceLock<Regex> = OnceLock::new();
        let natural_re = NATURAL_RE.get_or_init(|| {
            Regex::new(r"\b(?:Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:tember)?|Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s+\d{1,2},?\s+\d{4}\b").unwrap()
        });

        for m in natural_re.find_iter(text) {
            let raw = m.as_str().to_string();
            dates.push(DateEntity {
                raw: raw.clone(),
                parsed: None, // Parsing natural language dates would require chrono-english or similar
                is_relative: false,
                is_deadline: Self::is_deadline_context(&raw, text),
            });
        }

        // Relative dates
        static RELATIVE_RE: OnceLock<Regex> = OnceLock::new();
        let relative_re = RELATIVE_RE.get_or_init(|| {
            Regex::new(r"\b(?:today|tomorrow|yesterday|next\s+(?:week|month|monday|tuesday|wednesday|thursday|friday|saturday|sunday)|last\s+(?:week|month|monday|tuesday|wednesday|thursday|friday|saturday|sunday))\b").unwrap()
        });

        for m in relative_re.find_iter(text) {
            let raw = m.as_str().to_string();
            dates.push(DateEntity {
                raw: raw.clone(),
                parsed: None, // Would require more complex logic to resolve
                is_relative: true,
                is_deadline: Self::is_deadline_context(&raw, text),
            });
        }

        dates
    }

    /// Check if date appears in deadline context
    fn is_deadline_context(date: &str, text: &str) -> bool {
        let lower_text = text.to_lowercase();
        let lower_date = date.to_lowercase();

        // Find all occurrences, not just first
        for (byte_idx, _) in lower_text.match_indices(&lower_date) {
            // Get character-safe bounds
            let start = lower_text[..byte_idx]
                .char_indices()
                .rev()
                .take(50)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            let end = lower_text[byte_idx..]
                .char_indices()
                .take(date.len() + 50)
                .last()
                .map(|(i, _)| byte_idx + i + 1)
                .unwrap_or(lower_text.len());
            let context = &lower_text[start..end.min(lower_text.len())];

            // Check deadline keywords in context
            if context.contains("deadline")
                || context.contains("due")
                || context.contains("by ")
                || context.contains("before")
                || context.contains("must")
            {
                return true;
            }
        }
        false
    }

    /// Extract code references (functions, files with line numbers, etc.)
    pub fn extract_code_refs(text: &str) -> Vec<CodeReference> {
        let mut refs = Vec::new();

        // File with line number: file.rs:123 or file.rs:123:45
        static FILE_LINE_RE: OnceLock<Regex> = OnceLock::new();
        let file_line_re = FILE_LINE_RE.get_or_init(|| {
            Regex::new(r"\b([a-zA-Z0-9_/.-]+\.(?:rs|js|ts|py|go|java|cpp|c|h|rb|php|swift|kt|scala|sql)):(\d+)(?::(\d+))?\b").unwrap()
        });

        for cap in file_line_re.captures_iter(text) {
            let file = cap.get(1).unwrap().as_str();
            let line = cap.get(2).unwrap().as_str().parse().ok();
            let ext = file.split('.').next_back().unwrap_or("");

            refs.push(CodeReference {
                name: file.to_string(),
                kind: CodeRefKind::File,
                file_path: Some(file.to_string()),
                line_number: line,
                language: Some(ext.to_string()),
            });
        }

        // Backtick code references: `function_name` or `Class::method`
        static BACKTICK_RE: OnceLock<Regex> = OnceLock::new();
        let backtick_re = BACKTICK_RE.get_or_init(|| {
            Regex::new(r#"`([a-zA-Z_][a-zA-Z0-9_]*(?:::[a-zA-Z_][a-zA-Z0-9_]*)?)`"#).unwrap()
        });

        for cap in backtick_re.captures_iter(text) {
            let name = cap.get(1).unwrap().as_str();
            let kind = if name.contains("::") {
                CodeRefKind::Method
            } else if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                CodeRefKind::Class
            } else {
                CodeRefKind::Function
            };

            refs.push(CodeReference {
                name: name.to_string(),
                kind,
                file_path: None,
                line_number: None,
                language: None,
            });
        }

        refs
    }

    /// Extract file paths (absolute, relative, file://)
    pub fn extract_file_paths(text: &str) -> Vec<String> {
        let mut paths = Vec::new();

        // Absolute Unix paths
        static UNIX_ABS_RE: OnceLock<Regex> = OnceLock::new();
        let unix_abs_re =
            UNIX_ABS_RE.get_or_init(|| Regex::new(r"/[a-zA-Z0-9_./-]{1,500}").unwrap());

        for m in unix_abs_re.find_iter(text) {
            let path = m.as_str();
            // Filter out likely false positives (URLs, single /word, etc.)
            if path.len() > 2 && path.matches('/').count() >= 1 && !path.starts_with("//") {
                paths.push(path.to_string());
            }
        }

        // Relative paths starting with ./ or ../
        static REL_RE: OnceLock<Regex> = OnceLock::new();
        let rel_re = REL_RE.get_or_init(|| {
            Regex::new(r"\./[a-zA-Z0-9_./-]{1,500}|\.\./[a-zA-Z0-9_./-]{1,500}").unwrap()
        });

        for m in rel_re.find_iter(text) {
            paths.push(m.as_str().to_string());
        }

        // file:// URLs
        static FILE_URL_RE: OnceLock<Regex> = OnceLock::new();
        let file_url_re = FILE_URL_RE.get_or_init(|| Regex::new(r#"file://[^\s<>"']+"#).unwrap());

        for m in file_url_re.find_iter(text) {
            paths.push(m.as_str().to_string());
        }

        // Deduplicate
        paths.sort();
        paths.dedup();
        paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Email extraction tests
    #[test]
    fn test_extract_emails_single() {
        let text = "Contact john.doe@example.com for more info";
        let emails = EntityExtractor::extract_emails(text);

        assert_eq!(emails.len(), 1);
        assert_eq!(emails[0].email, Some("john.doe@example.com".to_string()));
        assert_eq!(emails[0].name, "john.doe");
    }

    #[test]
    fn test_extract_emails_multiple() {
        let text = "Send to alice@company.com and bob@company.org";
        let emails = EntityExtractor::extract_emails(text);

        assert_eq!(emails.len(), 2);
        assert!(
            emails
                .iter()
                .any(|e| e.email.as_ref().unwrap() == "alice@company.com")
        );
        assert!(
            emails
                .iter()
                .any(|e| e.email.as_ref().unwrap() == "bob@company.org")
        );
    }

    #[test]
    fn test_extract_emails_none() {
        let text = "No emails in this text";
        let emails = EntityExtractor::extract_emails(text);
        assert!(emails.is_empty());
    }

    // URL extraction tests
    #[test]
    fn test_extract_urls_github() {
        let text = "See https://github.com/user/repo for details";
        let urls = EntityExtractor::extract_urls(text);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "https://github.com/user/repo");
        assert_eq!(urls[0].url_type, UrlType::GitHub);
    }

    #[test]
    fn test_extract_urls_jira() {
        let text = "Ticket: https://company.atlassian.net/browse/PROJ-123";
        let urls = EntityExtractor::extract_urls(text);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url_type, UrlType::Jira);
    }

    #[test]
    fn test_extract_urls_generic() {
        let text = "Visit https://example.com for more";
        let urls = EntityExtractor::extract_urls(text);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "https://example.com");
        assert_eq!(urls[0].url_type, UrlType::Web);
    }

    #[test]
    fn test_extract_urls_multiple_types() {
        let text = "Check https://github.com/repo and https://docs.example.com/guide";
        let urls = EntityExtractor::extract_urls(text);

        assert_eq!(urls.len(), 2);
        assert!(urls.iter().any(|u| u.url_type == UrlType::GitHub));
        assert!(urls.iter().any(|u| u.url_type == UrlType::Documentation));
    }

    // Date extraction tests
    #[test]
    fn test_extract_dates_iso() {
        let text = "Deadline is 2024-01-15";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        let date = &dates[0];
        assert_eq!(date.raw, "2024-01-15");
        assert!(!date.is_relative);
        assert!(date.parsed.is_some());
    }

    #[test]
    fn test_extract_dates_iso_with_time() {
        let text = "Meeting at 2024-01-15T14:30:00Z";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        let date = &dates[0];
        assert_eq!(date.raw, "2024-01-15T14:30:00Z");
        assert!(date.parsed.is_some());
    }

    #[test]
    fn test_extract_dates_natural() {
        let text = "Due on January 15, 2024";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        let date = &dates[0];
        assert_eq!(date.raw, "January 15, 2024");
        assert!(!date.is_relative);
    }

    #[test]
    fn test_extract_dates_natural_short() {
        let text = "Meeting Jan 5, 2024";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        assert_eq!(dates[0].raw, "Jan 5, 2024");
    }

    #[test]
    fn test_extract_dates_relative() {
        let text = "Let's meet next week";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        let date = &dates[0];
        assert_eq!(date.raw, "next week");
        assert!(date.is_relative);
    }

    #[test]
    fn test_extract_dates_deadline_context() {
        let text = "Deadline is 2024-01-15";
        let dates = EntityExtractor::extract_dates(text);

        assert!(!dates.is_empty());
        assert!(dates[0].is_deadline);
    }

    // Code reference tests
    #[test]
    fn test_extract_code_refs_function() {
        let text = "Call the `calculate_total` function";
        let refs = EntityExtractor::extract_code_refs(text);

        assert!(!refs.is_empty());
        let code_ref = &refs[0];
        assert_eq!(code_ref.name, "calculate_total");
        assert_eq!(code_ref.kind, CodeRefKind::Function);
    }

    #[test]
    fn test_extract_code_refs_method() {
        let text = "Use `Database::connect` method";
        let refs = EntityExtractor::extract_code_refs(text);

        assert!(!refs.is_empty());
        let code_ref = &refs[0];
        assert_eq!(code_ref.name, "Database::connect");
        assert_eq!(code_ref.kind, CodeRefKind::Method);
    }

    #[test]
    fn test_extract_code_refs_file_line() {
        let text = "Error in main.rs:123";
        let refs = EntityExtractor::extract_code_refs(text);

        assert!(!refs.is_empty());
        let code_ref = &refs[0];
        assert_eq!(code_ref.name, "main.rs");
        assert_eq!(code_ref.kind, CodeRefKind::File);
        assert_eq!(code_ref.file_path, Some("main.rs".to_string()));
        assert_eq!(code_ref.line_number, Some(123));
        assert_eq!(code_ref.language, Some("rs".to_string()));
    }

    #[test]
    fn test_extract_code_refs_file_line_column() {
        let text = "See utils/helpers.ts:45:12";
        let refs = EntityExtractor::extract_code_refs(text);

        assert!(!refs.is_empty());
        let code_ref = &refs[0];
        assert_eq!(code_ref.file_path, Some("utils/helpers.ts".to_string()));
        assert_eq!(code_ref.line_number, Some(45));
    }

    #[test]
    fn test_extract_code_refs_class() {
        let text = "The `UserManager` class handles auth";
        let refs = EntityExtractor::extract_code_refs(text);

        assert!(!refs.is_empty());
        let code_ref = &refs[0];
        assert_eq!(code_ref.name, "UserManager");
        assert_eq!(code_ref.kind, CodeRefKind::Class);
    }

    // File path tests
    #[test]
    fn test_extract_file_paths_absolute() {
        let text = "Check /usr/local/bin/myapp";
        let paths = EntityExtractor::extract_file_paths(text);

        assert!(!paths.is_empty());
        assert!(paths.contains(&"/usr/local/bin/myapp".to_string()));
    }

    #[test]
    fn test_extract_file_paths_relative() {
        let text = "See ./src/main.rs for implementation";
        let paths = EntityExtractor::extract_file_paths(text);

        assert!(!paths.is_empty());
        assert!(paths.contains(&"./src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_file_paths_parent() {
        let text = "Import from ../utils/helpers.js";
        let paths = EntityExtractor::extract_file_paths(text);

        assert!(!paths.is_empty());
        assert!(paths.contains(&"../utils/helpers.js".to_string()));
    }

    #[test]
    fn test_extract_file_paths_file_url() {
        let text = "Open file:///home/user/document.txt";
        let paths = EntityExtractor::extract_file_paths(text);

        assert!(!paths.is_empty());
        assert!(paths.contains(&"file:///home/user/document.txt".to_string()));
    }

    #[test]
    fn test_extract_all_entities() {
        let text = r#"
            Contact alice@example.com about the bug in main.rs:45.
            See https://github.com/user/repo for details.
            Deadline: 2024-01-15
            Check ./src/utils for the `calculate_total` function.
        "#;

        let entities = EntityExtractor::extract(text);

        assert!(!entities.people.is_empty());
        assert!(!entities.urls.is_empty());
        assert!(!entities.dates.is_empty());
        assert!(!entities.code_refs.is_empty());
        assert!(!entities.file_paths.is_empty());
    }
}

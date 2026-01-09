//! Intelligent content sizing logic
//!
//! Determines the optimal fetch strategy based on estimated content size
//! and available token budget.

use super::{DEFAULT_TOKEN_BUDGET, FetchStrategy, estimate_tokens_from_bytes};

/// Threshold multiplier for metadata-only fallback
/// If estimated tokens > budget * this value, use metadata only
const METADATA_ONLY_THRESHOLD_MULTIPLIER: usize = 10;

/// Decide the fetch strategy based on estimated size vs budget
///
/// # Strategy Selection
/// - **Full**: Estimated tokens <= budget (fetch everything)
/// - **Partial**: Estimated tokens <= budget * 10 (fetch up to budget)
/// - **MetadataOnly**: Estimated tokens > budget * 10 (too large, just metadata)
pub fn decide_fetch_strategy(estimated_tokens: usize, budget: usize) -> FetchStrategy {
    if estimated_tokens <= budget {
        FetchStrategy::Full
    } else if estimated_tokens <= budget * METADATA_ONLY_THRESHOLD_MULTIPLIER {
        FetchStrategy::Partial { max_tokens: budget }
    } else {
        FetchStrategy::MetadataOnly
    }
}

/// Token budget tracker for progressive fetching
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// Total token budget
    total: usize,
    /// Tokens used so far
    used: usize,
    /// Items tracked (id, tokens)
    items: Vec<(String, usize)>,
}

impl TokenBudget {
    /// Create a new token budget
    pub fn new(budget: usize) -> Self {
        Self {
            total: budget,
            used: 0,
            items: Vec::new(),
        }
    }

    /// Create with default budget
    pub fn default_budget() -> Self {
        Self::new(DEFAULT_TOKEN_BUDGET)
    }

    /// Get remaining tokens
    pub fn remaining(&self) -> usize {
        self.total.saturating_sub(self.used)
    }

    /// Check if tokens can fit in budget
    pub fn can_fit(&self, tokens: usize) -> bool {
        self.used + tokens <= self.total
    }

    /// Try to add an item to the budget
    ///
    /// Returns true if the item was added, false if it doesn't fit
    pub fn try_add(&mut self, id: impl Into<String>, tokens: usize) -> bool {
        if self.can_fit(tokens) {
            self.used += tokens;
            self.items.push((id.into(), tokens));
            true
        } else {
            false
        }
    }

    /// Get total used tokens
    pub fn used(&self) -> usize {
        self.used
    }

    /// Get total budget
    pub fn total(&self) -> usize {
        self.total
    }

    /// Get number of items
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Get utilization percentage (0.0 - 1.0)
    pub fn utilization(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64
        }
    }

    /// Check if budget is exhausted (>= 95% used)
    pub fn is_exhausted(&self) -> bool {
        self.utilization() >= 0.95
    }
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self::default_budget()
    }
}

/// Priority score for content items
///
/// Higher scores = higher priority for inclusion
#[derive(Debug, Clone)]
pub struct PriorityScore {
    /// The item path/id
    pub path: String,
    /// Total priority score (0-1000)
    pub score: u32,
    /// Estimated tokens
    pub estimated_tokens: usize,
}

/// Calculate priority score for a file path
///
/// Uses heuristics based on:
/// - File type (documentation, code, config, tests)
/// - Directory depth (shallower = higher priority)
/// - Special files (README, CLAUDE.md, etc.)
/// - File size (smaller = higher priority for initial fetch)
pub fn calculate_file_priority(path: &str, size_bytes: Option<usize>) -> u32 {
    let mut score = 100u32;

    // Get extension
    let extension = path.rsplit('.').next().unwrap_or("");
    let lowercase_path = path.to_lowercase();

    // Priority by file type
    score += match extension.to_lowercase().as_str() {
        // Documentation - highest priority
        "md" | "txt" | "rst" => 50,
        // Main code files
        "rs" | "py" | "ts" | "js" | "go" | "java" | "c" | "cpp" | "h" => 40,
        // Config files
        "toml" | "yaml" | "yml" | "json" => 30,
        // Test files (lower priority)
        _ if lowercase_path.contains("test") || lowercase_path.contains("spec") => 15,
        // Other
        _ => 0,
    };

    // Priority by directory depth (shallower = higher)
    let depth = path.matches('/').count();
    score += 10u32.saturating_sub(depth as u32 * 2);

    // Special files get highest priority
    if lowercase_path.contains("readme") {
        score += 100;
    }
    if lowercase_path.contains("claude") {
        score += 150;
    }
    if lowercase_path.ends_with("cargo.toml") || lowercase_path.ends_with("package.json") {
        score += 80;
    }
    if lowercase_path.ends_with("lib.rs") || lowercase_path.ends_with("main.rs") {
        score += 60;
    }
    if lowercase_path.ends_with("mod.rs") || lowercase_path.ends_with("index.ts") {
        score += 40;
    }

    // Penalize large files
    if let Some(size) = size_bytes {
        if size > 100_000 {
            score = score.saturating_sub(30);
        } else if size > 50_000 {
            score = score.saturating_sub(15);
        } else if size < 10_000 {
            score += 20;
        }
    }

    // Penalize generated/build files
    if lowercase_path.contains("node_modules")
        || lowercase_path.contains("target/")
        || lowercase_path.contains("dist/")
        || lowercase_path.contains("build/")
        || lowercase_path.contains(".min.")
    {
        score = 0;
    }

    score
}

/// Sort paths by priority (highest first)
pub fn prioritize_paths(paths: &[(String, Option<usize>)]) -> Vec<PriorityScore> {
    let mut scores: Vec<PriorityScore> = paths
        .iter()
        .map(|(path, size)| {
            let score = calculate_file_priority(path, *size);
            let estimated_tokens = size.map(estimate_tokens_from_bytes).unwrap_or(0);
            PriorityScore {
                path: path.clone(),
                score,
                estimated_tokens,
            }
        })
        .collect();

    // Sort by score descending
    scores.sort_by(|a, b| b.score.cmp(&a.score));

    scores
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decide_strategy_full() {
        let strategy = decide_fetch_strategy(50_000, 100_000);
        assert!(matches!(strategy, FetchStrategy::Full));
    }

    #[test]
    fn test_decide_strategy_partial() {
        let strategy = decide_fetch_strategy(500_000, 100_000);
        assert!(matches!(
            strategy,
            FetchStrategy::Partial {
                max_tokens: 100_000
            }
        ));
    }

    #[test]
    fn test_decide_strategy_metadata_only() {
        let strategy = decide_fetch_strategy(5_000_000, 100_000);
        assert!(matches!(strategy, FetchStrategy::MetadataOnly));
    }

    #[test]
    fn test_token_budget_new() {
        let budget = TokenBudget::new(10_000);
        assert_eq!(budget.total(), 10_000);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.remaining(), 10_000);
    }

    #[test]
    fn test_token_budget_try_add() {
        let mut budget = TokenBudget::new(100);

        assert!(budget.try_add("item1", 30));
        assert_eq!(budget.used(), 30);
        assert_eq!(budget.remaining(), 70);

        assert!(budget.try_add("item2", 50));
        assert_eq!(budget.used(), 80);

        // This should fail - doesn't fit
        assert!(!budget.try_add("item3", 50));
        assert_eq!(budget.used(), 80); // Unchanged

        // But this should fit
        assert!(budget.try_add("item4", 20));
        assert_eq!(budget.used(), 100);
    }

    #[test]
    fn test_token_budget_utilization() {
        let mut budget = TokenBudget::new(100);
        assert_eq!(budget.utilization(), 0.0);

        budget.try_add("item", 50);
        assert!((budget.utilization() - 0.5).abs() < 0.01);

        budget.try_add("item2", 45);
        assert!(budget.is_exhausted()); // 95%
    }

    #[test]
    fn test_file_priority_readme() {
        let score = calculate_file_priority("README.md", None);
        let regular_score = calculate_file_priority("src/lib.rs", None);

        assert!(score > regular_score, "README should have higher priority");
    }

    #[test]
    fn test_file_priority_claude() {
        let score = calculate_file_priority("CLAUDE.md", None);
        let readme_score = calculate_file_priority("README.md", None);

        assert!(
            score > readme_score,
            "CLAUDE.md should have highest priority"
        );
    }

    #[test]
    fn test_file_priority_depth() {
        let shallow = calculate_file_priority("src/lib.rs", None);
        let deep = calculate_file_priority("src/foo/bar/baz/lib.rs", None);

        assert!(
            shallow > deep,
            "Shallower files should have higher priority"
        );
    }

    #[test]
    fn test_file_priority_test_files() {
        let code = calculate_file_priority("src/lib.rs", None);
        let test = calculate_file_priority("src/lib.test.rs", None);

        assert!(code > test, "Test files should have lower priority");
    }

    #[test]
    fn test_file_priority_node_modules() {
        let score = calculate_file_priority("node_modules/foo/index.js", None);
        assert_eq!(score, 0, "node_modules should be excluded");
    }

    #[test]
    fn test_prioritize_paths() {
        let paths = vec![
            ("src/lib.rs".to_string(), Some(1000)),
            ("README.md".to_string(), Some(500)),
            ("CLAUDE.md".to_string(), Some(200)),
            ("node_modules/foo.js".to_string(), Some(100)),
        ];

        let prioritized = prioritize_paths(&paths);

        assert_eq!(prioritized[0].path, "CLAUDE.md");
        assert_eq!(prioritized[1].path, "README.md");
        assert_eq!(prioritized.last().unwrap().path, "node_modules/foo.js");
    }
}

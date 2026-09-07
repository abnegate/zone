//! Typed configuration and bounds for a code quality evaluation.

use std::time::Duration;

use super::category::EvalCategory;

pub const ENABLED_VARIABLE: &str = "ZONE_TASK_EVALUATION";
pub const CATEGORIES_VARIABLE: &str = "ZONE_TASK_EVALUATION_CATEGORIES";
pub const TOOL_TIMEOUT_VARIABLE: &str = "ZONE_TASK_EVALUATION_TOOL_TIMEOUT_SECONDS";
pub const BUDGET_VARIABLE: &str = "ZONE_TASK_EVALUATION_BUDGET_SECONDS";

const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(300);
const DEFAULT_TOTAL_BUDGET: Duration = Duration::from_secs(900);
const MAXIMUM_TOOL_TIMEOUT: Duration = Duration::from_secs(1800);
const MAXIMUM_TOTAL_BUDGET: Duration = Duration::from_secs(3600);

/// Bytes read from each of a tool's output streams before it is cut off.
const DEFAULT_CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

/// Bytes of a tool's output kept on the snapshot that reaches the database.
const DEFAULT_STORED_OUTPUT_LIMIT: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CategorySelection {
    pub test: bool,
    pub lint: bool,
    pub typecheck: bool,
    pub build: bool,
    pub coverage: bool,
}

impl CategorySelection {
    pub const NONE: Self = Self {
        test: false,
        lint: false,
        typecheck: false,
        build: false,
        coverage: false,
    };

    pub const ALL: Self = Self {
        test: true,
        lint: true,
        typecheck: true,
        build: true,
        coverage: true,
    };

    pub fn includes(&self, category: EvalCategory) -> bool {
        match category {
            EvalCategory::Test => self.test,
            EvalCategory::Lint => self.lint,
            EvalCategory::Typecheck => self.typecheck,
            EvalCategory::Build => self.build,
            EvalCategory::Coverage => self.coverage,
        }
    }

    fn enable(&mut self, category: EvalCategory) {
        match category {
            EvalCategory::Test => self.test = true,
            EvalCategory::Lint => self.lint = true,
            EvalCategory::Typecheck => self.typecheck = true,
            EvalCategory::Build => self.build = true,
            EvalCategory::Coverage => self.coverage = true,
        }
    }

    pub fn parse(list: &str) -> Self {
        let mut selection = Self::NONE;
        for token in list.split(',') {
            let token = token.trim().to_ascii_lowercase();
            if token == "all" {
                return Self::ALL;
            }
            if let Some(category) = EvalCategory::ALL.iter().find(|c| c.as_str() == token) {
                selection.enable(*category);
            }
        }
        selection
    }
}

impl Default for CategorySelection {
    /// Builds and coverage runs cost far more than the signal they add to a
    /// single task run, so they are opt-in.
    fn default() -> Self {
        Self {
            test: true,
            lint: true,
            typecheck: true,
            build: false,
            coverage: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationSettings {
    pub enabled: bool,
    pub categories: CategorySelection,
    pub tool_timeout: Duration,
    pub total_budget: Duration,
    pub capture_limit: usize,
    pub stored_output_limit: usize,
}

impl Default for EvaluationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            categories: CategorySelection::default(),
            tool_timeout: DEFAULT_TOOL_TIMEOUT,
            total_budget: DEFAULT_TOTAL_BUDGET,
            capture_limit: DEFAULT_CAPTURE_LIMIT,
            stored_output_limit: DEFAULT_STORED_OUTPUT_LIMIT,
        }
    }
}

/// The raw environment a set of settings is resolved from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationEnvironment {
    pub enabled: Option<String>,
    pub categories: Option<String>,
    pub tool_timeout_seconds: Option<String>,
    pub budget_seconds: Option<String>,
}

impl EvaluationEnvironment {
    pub fn from_process() -> Self {
        Self {
            enabled: std::env::var(ENABLED_VARIABLE).ok(),
            categories: std::env::var(CATEGORIES_VARIABLE).ok(),
            tool_timeout_seconds: std::env::var(TOOL_TIMEOUT_VARIABLE).ok(),
            budget_seconds: std::env::var(BUDGET_VARIABLE).ok(),
        }
    }
}

impl EvaluationSettings {
    pub fn resolve(environment: &EvaluationEnvironment) -> Self {
        let defaults = Self::default();
        Self {
            enabled: environment
                .enabled
                .as_deref()
                .map(parse_flag)
                .unwrap_or(defaults.enabled),
            categories: environment
                .categories
                .as_deref()
                .map(CategorySelection::parse)
                .unwrap_or(defaults.categories),
            tool_timeout: parse_duration(
                environment.tool_timeout_seconds.as_deref(),
                defaults.tool_timeout,
                MAXIMUM_TOOL_TIMEOUT,
            ),
            total_budget: parse_duration(
                environment.budget_seconds.as_deref(),
                defaults.total_budget,
                MAXIMUM_TOTAL_BUDGET,
            ),
            ..defaults
        }
    }

    pub fn from_process_environment() -> Self {
        Self::resolve(&EvaluationEnvironment::from_process())
    }

    /// A tool never gets more time than the budget that is left for the whole run.
    pub fn slice(&self, remaining: Duration) -> Duration {
        self.tool_timeout.min(remaining)
    }
}

fn parse_flag(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_duration(raw: Option<&str>, fallback: Duration, ceiling: Duration) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(fallback)
        .min(ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_is_off_unless_it_is_asked_for() {
        let settings = EvaluationSettings::resolve(&EvaluationEnvironment::default());
        assert!(
            !settings.enabled,
            "running a project's test suite is expensive, so it must be opt-in"
        );
    }

    #[test]
    fn accepts_the_usual_spellings_of_true() {
        for raw in ["1", "true", "TRUE", "yes", " on "] {
            let settings = EvaluationSettings::resolve(&EvaluationEnvironment {
                enabled: Some(raw.to_string()),
                ..EvaluationEnvironment::default()
            });
            assert!(settings.enabled, "{raw} should enable evaluation");
        }
        for raw in ["0", "false", "no", "", "maybe"] {
            let settings = EvaluationSettings::resolve(&EvaluationEnvironment {
                enabled: Some(raw.to_string()),
                ..EvaluationEnvironment::default()
            });
            assert!(!settings.enabled, "{raw} should not enable evaluation");
        }
    }

    #[test]
    fn defaults_skip_build_and_coverage() {
        let categories = CategorySelection::default();
        assert!(categories.includes(EvalCategory::Test));
        assert!(categories.includes(EvalCategory::Lint));
        assert!(categories.includes(EvalCategory::Typecheck));
        assert!(!categories.includes(EvalCategory::Build));
        assert!(!categories.includes(EvalCategory::Coverage));
    }

    #[test]
    fn parses_a_category_list() {
        let categories = CategorySelection::parse("test, coverage");
        assert!(categories.includes(EvalCategory::Test));
        assert!(categories.includes(EvalCategory::Coverage));
        assert!(!categories.includes(EvalCategory::Lint));
    }

    #[test]
    fn parses_the_all_shorthand() {
        assert_eq!(CategorySelection::parse("all"), CategorySelection::ALL);
        assert_eq!(CategorySelection::parse("test,all"), CategorySelection::ALL);
    }

    #[test]
    fn ignores_unknown_category_names() {
        assert_eq!(
            CategorySelection::parse("nonsense"),
            CategorySelection::NONE
        );
        let categories = CategorySelection::parse("nonsense,lint");
        assert!(categories.includes(EvalCategory::Lint));
        assert!(!categories.includes(EvalCategory::Test));
    }

    #[test]
    fn clamps_timeouts_to_a_ceiling() {
        let settings = EvaluationSettings::resolve(&EvaluationEnvironment {
            tool_timeout_seconds: Some("99999".to_string()),
            budget_seconds: Some("99999".to_string()),
            ..EvaluationEnvironment::default()
        });
        assert_eq!(settings.tool_timeout, MAXIMUM_TOOL_TIMEOUT);
        assert_eq!(settings.total_budget, MAXIMUM_TOTAL_BUDGET);
    }

    #[test]
    fn falls_back_to_defaults_for_unparsable_timeouts() {
        let settings = EvaluationSettings::resolve(&EvaluationEnvironment {
            tool_timeout_seconds: Some("soon".to_string()),
            budget_seconds: Some("0".to_string()),
            ..EvaluationEnvironment::default()
        });
        assert_eq!(settings.tool_timeout, DEFAULT_TOOL_TIMEOUT);
        assert_eq!(settings.total_budget, DEFAULT_TOTAL_BUDGET);
    }

    #[test]
    fn reads_explicit_timeouts() {
        let settings = EvaluationSettings::resolve(&EvaluationEnvironment {
            tool_timeout_seconds: Some("30".to_string()),
            budget_seconds: Some("120".to_string()),
            ..EvaluationEnvironment::default()
        });
        assert_eq!(settings.tool_timeout, Duration::from_secs(30));
        assert_eq!(settings.total_budget, Duration::from_secs(120));
    }

    #[test]
    fn a_tool_never_outlives_the_remaining_budget() {
        let settings = EvaluationSettings::default();
        assert_eq!(
            settings.slice(Duration::from_secs(5)),
            Duration::from_secs(5)
        );
        assert_eq!(
            settings.slice(Duration::from_secs(10_000)),
            settings.tool_timeout
        );
    }

    #[test]
    fn stored_output_is_much_smaller_than_captured_output() {
        let settings = EvaluationSettings::default();
        assert!(
            settings.stored_output_limit < settings.capture_limit,
            "the full stream is needed for parsing, but only a tail is worth persisting"
        );
    }
}

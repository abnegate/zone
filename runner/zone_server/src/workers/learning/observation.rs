//! Item 13a: reading conventions out of the files a run changed.
//!
//! Every diff is a statement about how this repository is laid out: where its tests
//! live, how its files are named, what belongs in which directory. One diff is not
//! evidence of anything — [`super::convention`] is what decides when a pattern has been
//! confirmed often enough to believe — but this is where the raw statements come from.
//!
//! Only unambiguous observations are emitted. A file called `main.rs` is equally
//! consistent with snake_case, kebab-case and camelCase, so it contributes nothing
//! rather than casting a vote it cannot justify.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

const INLINE_TEST_MARKERS: [&str; 3] = ["#[cfg(test)]", "#[cfg(all(test", "#[test]"];
const TEST_DIRECTORIES: [&str; 4] = ["tests", "test", "__tests__", "spec"];
const ROOT_SCOPE: &str = ".";

/// What a run did to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeType {
    Create,
    Modify,
    Delete,
}

impl ChangeType {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "create" => Some(ChangeType::Create),
            "modify" => Some(ChangeType::Modify),
            "delete" => Some(ChangeType::Delete),
            _ => None,
        }
    }
}

/// One file a run changed, with the added text when it is available.
#[derive(Debug, Clone, PartialEq)]
pub struct FileChange {
    pub run_id: Uuid,
    pub path: String,
    pub change_type: ChangeType,
    /// The diff body, used only to spot tests written inside a source file.
    pub diff: Option<String>,
}

/// The aspects of a repository's layout the loop can learn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConventionKind {
    /// How files in a directory are named. Scope is the directory.
    FileNaming,
    /// What kind of file a directory holds. Scope is the directory.
    DirectoryContent,
    /// Where a language's tests live. Scope is the file extension.
    TestPlacement,
    /// How a language's test files are named. Scope is the file extension.
    TestNaming,
}

impl ConventionKind {
    pub const ALL: [ConventionKind; 4] = [
        ConventionKind::FileNaming,
        ConventionKind::DirectoryContent,
        ConventionKind::TestPlacement,
        ConventionKind::TestNaming,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ConventionKind::FileNaming => "file_naming",
            ConventionKind::DirectoryContent => "directory_content",
            ConventionKind::TestPlacement => "test_placement",
            ConventionKind::TestNaming => "test_naming",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// What the convention reads as once it is believed.
    pub fn statement(self, scope: &str, value: &str) -> String {
        match self {
            ConventionKind::FileNaming => {
                format!("Files in `{scope}` are named in {value}.")
            }
            ConventionKind::DirectoryContent => {
                format!("`{scope}` holds `.{value}` files.")
            }
            ConventionKind::TestPlacement => match value {
                "inline_module" => {
                    format!("`.{scope}` tests live inside the file they test.")
                }
                "sibling_file" => {
                    format!("`.{scope}` tests live in a test file beside the code.")
                }
                _ => format!("`.{scope}` tests live in a separate test tree."),
            },
            ConventionKind::TestNaming => {
                format!("`.{scope}` test files are named `{value}`.")
            }
        }
    }
}

impl fmt::Display for ConventionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One vote, from one run, that a scope follows a value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConventionSignal {
    pub kind: ConventionKind,
    /// A directory or a file extension, depending on the kind.
    pub scope: String,
    pub value: String,
    pub run_id: Uuid,
}

/// How identifiers are cased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamingStyle {
    SnakeCase,
    KebabCase,
    CamelCase,
    PascalCase,
    ScreamingSnakeCase,
}

impl NamingStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            NamingStyle::SnakeCase => "snake_case",
            NamingStyle::KebabCase => "kebab-case",
            NamingStyle::CamelCase => "camelCase",
            NamingStyle::PascalCase => "PascalCase",
            NamingStyle::ScreamingSnakeCase => "SCREAMING_SNAKE_CASE",
        }
    }
}

impl fmt::Display for NamingStyle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The directory a path sits in, or [`ROOT_SCOPE`] for a file at the repository root.
pub fn directory(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent,
        _ => ROOT_SCOPE,
    }
}

/// The file name without any directories.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The last extension of a file, lowercased, if it has one.
pub fn extension(path: &str) -> Option<String> {
    let name = file_name(path);
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() || extension.is_empty() || !extension.chars().all(char::is_alphanumeric) {
        return None;
    }
    Some(extension.to_ascii_lowercase())
}

/// The file name with every extension stripped, so `button.test.tsx` is `button`.
pub fn stem(path: &str) -> &str {
    let name = file_name(path);
    name.split_once('.').map_or(name, |(stem, _)| stem)
}

/// The casing of a name, or `None` when the name is consistent with several.
pub fn naming_style(name: &str) -> Option<NamingStyle> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }

    let has_underscore = name.contains('_');
    let has_hyphen = name.contains('-');
    if has_underscore && has_hyphen {
        return None;
    }

    let letters: Vec<char> = name.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return None;
    }
    let has_upper = letters.iter().any(|c| c.is_uppercase());
    let has_lower = letters.iter().any(|c| c.is_lowercase());

    if has_underscore {
        return match (has_upper, has_lower) {
            (false, true) => Some(NamingStyle::SnakeCase),
            (true, false) => Some(NamingStyle::ScreamingSnakeCase),
            _ => None,
        };
    }
    if has_hyphen {
        return has_lower
            .then_some(NamingStyle::KebabCase)
            .filter(|_| !has_upper);
    }
    if !has_upper {
        return None;
    }
    if letters[0].is_uppercase() {
        return has_lower.then_some(NamingStyle::PascalCase);
    }
    Some(NamingStyle::CamelCase)
}

/// Whether a path sits under a directory dedicated to tests.
pub fn in_test_directory(path: &str) -> bool {
    path.split('/')
        .rev()
        .skip(1)
        .any(|segment| TEST_DIRECTORIES.contains(&segment.to_ascii_lowercase().as_str()))
}

/// How a test file announces itself, if it does.
pub fn test_naming(path: &str) -> Option<&'static str> {
    let name = file_name(path).to_ascii_lowercase();
    let stem = stem(path).to_ascii_lowercase();

    if name.contains(".test.") {
        Some("name.test.ext")
    } else if name.contains(".spec.") {
        Some("name.spec.ext")
    } else if stem.ends_with("_test") {
        Some("name_test.ext")
    } else if stem.ends_with("_spec") {
        Some("name_spec.ext")
    } else if stem.starts_with("test_") {
        Some("test_name.ext")
    } else {
        None
    }
}

/// Whether this file is a test, by name or by where it lives.
pub fn is_test_file(path: &str) -> bool {
    test_naming(path).is_some() || in_test_directory(path)
}

fn added_lines(diff: &str) -> impl Iterator<Item = &str> {
    diff.lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
}

fn declares_inline_tests(diff: &str) -> bool {
    added_lines(diff).any(|line| {
        let trimmed = line.trim_start_matches('+').trim();
        INLINE_TEST_MARKERS
            .iter()
            .any(|marker| trimmed.starts_with(marker))
    })
}

/// Read every convention signal one changed file supports.
///
/// A deleted file says nothing about how the repository is arranged now, so it emits
/// nothing.
pub fn signals_for(change: &FileChange) -> Vec<ConventionSignal> {
    if change.change_type == ChangeType::Delete || change.path.trim().is_empty() {
        return Vec::new();
    }

    let mut signals = Vec::new();
    let path = change.path.trim();
    let mut push = |kind: ConventionKind, scope: String, value: String| {
        signals.push(ConventionSignal {
            kind,
            scope,
            value,
            run_id: change.run_id,
        });
    };

    if let Some(style) = naming_style(stem(path)) {
        push(
            ConventionKind::FileNaming,
            directory(path).to_string(),
            style.as_str().to_string(),
        );
    }

    if let Some(extension) = extension(path) {
        push(
            ConventionKind::DirectoryContent,
            directory(path).to_string(),
            extension.clone(),
        );

        let placement = if is_test_file(path) {
            Some(if in_test_directory(path) {
                "separate_tree"
            } else {
                "sibling_file"
            })
        } else if change.diff.as_deref().is_some_and(declares_inline_tests) {
            Some("inline_module")
        } else {
            None
        };

        if let Some(placement) = placement {
            push(
                ConventionKind::TestPlacement,
                extension.clone(),
                placement.to_string(),
            );
        }

        if let Some(naming) = test_naming(path) {
            push(ConventionKind::TestNaming, extension, naming.to_string());
        }
    }

    signals
}

/// Every signal a run's changed files support.
pub fn signals(changes: &[FileChange]) -> Vec<ConventionSignal> {
    changes.iter().flat_map(signals_for).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str) -> FileChange {
        FileChange {
            run_id: Uuid::from_u128(1),
            path: path.to_string(),
            change_type: ChangeType::Modify,
            diff: None,
        }
    }

    fn values(signals: &[ConventionSignal], kind: ConventionKind) -> Vec<(String, String)> {
        signals
            .iter()
            .filter(|signal| signal.kind == kind)
            .map(|signal| (signal.scope.clone(), signal.value.clone()))
            .collect()
    }

    #[test]
    fn paths_split_into_directory_name_and_extension() {
        assert_eq!(directory("src/db/knowledge.rs"), "src/db");
        assert_eq!(directory("README.md"), ".");
        assert_eq!(file_name("src/db/knowledge.rs"), "knowledge.rs");
        assert_eq!(extension("src/db/knowledge.rs").as_deref(), Some("rs"));
        assert_eq!(extension("Makefile"), None);
        assert_eq!(
            extension("src/components/Button.test.tsx").as_deref(),
            Some("tsx")
        );
        assert_eq!(stem("src/components/Button.test.tsx"), "Button");
    }

    #[test]
    fn naming_styles_are_recognised() {
        assert_eq!(naming_style("error_category"), Some(NamingStyle::SnakeCase));
        assert_eq!(naming_style("error-category"), Some(NamingStyle::KebabCase));
        assert_eq!(naming_style("errorCategory"), Some(NamingStyle::CamelCase));
        assert_eq!(naming_style("ErrorCategory"), Some(NamingStyle::PascalCase));
        assert_eq!(
            naming_style("MAX_RETRIES"),
            Some(NamingStyle::ScreamingSnakeCase)
        );
    }

    #[test]
    fn an_ambiguous_name_casts_no_vote() {
        for name in ["main", "index", "mod", "lib", "", "x"] {
            assert_eq!(
                naming_style(name),
                None,
                "{name:?} is consistent with several conventions and must not vote for one"
            );
        }
        assert_eq!(
            naming_style("mixed_style-name"),
            None,
            "a name using both separators supports neither convention"
        );
    }

    #[test]
    fn test_files_are_recognised_by_name_and_by_location() {
        assert!(is_test_file("tests/integration.rs"));
        assert!(is_test_file("src/api_test.go"));
        assert!(is_test_file("src/components/Button.test.tsx"));
        assert!(is_test_file("src/components/Button.spec.ts"));
        assert!(is_test_file("app/test_parser.py"));
        assert!(
            !is_test_file("src/contestant.rs"),
            "'contestant' is not a test"
        );
        assert!(!is_test_file("src/latest.rs"), "'latest' is not a test");
    }

    #[test]
    fn test_naming_patterns_are_distinguished() {
        assert_eq!(test_naming("src/api_test.go"), Some("name_test.ext"));
        assert_eq!(test_naming("web/Button.test.tsx"), Some("name.test.ext"));
        assert_eq!(test_naming("web/Button.spec.ts"), Some("name.spec.ext"));
        assert_eq!(test_naming("app/test_parser.py"), Some("test_name.ext"));
        assert_eq!(test_naming("src/parser.rs"), None);
    }

    #[test]
    fn a_deleted_file_teaches_nothing() {
        let removed = FileChange {
            change_type: ChangeType::Delete,
            ..change("src/db/old_thing.rs")
        };
        assert!(
            signals_for(&removed).is_empty(),
            "removing a file says nothing about how the repository is arranged now"
        );
    }

    #[test]
    fn a_source_file_supports_naming_and_directory_signals() {
        let signals = signals_for(&change("src/db/error_category.rs"));

        assert_eq!(
            values(&signals, ConventionKind::FileNaming),
            vec![("src/db".to_string(), "snake_case".to_string())]
        );
        assert_eq!(
            values(&signals, ConventionKind::DirectoryContent),
            vec![("src/db".to_string(), "rs".to_string())]
        );
        assert!(
            values(&signals, ConventionKind::TestPlacement).is_empty(),
            "an ordinary source file says nothing about where tests live"
        );
    }

    #[test]
    fn tests_in_a_dedicated_tree_are_told_apart_from_tests_beside_the_code() {
        let separate = signals_for(&change("tests/integration_flow.rs"));
        assert_eq!(
            values(&separate, ConventionKind::TestPlacement),
            vec![("rs".to_string(), "separate_tree".to_string())]
        );

        let sibling = signals_for(&change("internal/api/handler_test.go"));
        assert_eq!(
            values(&sibling, ConventionKind::TestPlacement),
            vec![("go".to_string(), "sibling_file".to_string())]
        );
    }

    #[test]
    fn a_test_module_added_inside_a_source_file_is_an_inline_convention() {
        let inline = FileChange {
            diff: Some(
                "@@ -1 +1,5 @@\n context\n+#[cfg(test)]\n+mod tests {\n+    use super::*;\n+}\n"
                    .to_string(),
            ),
            ..change("src/workers/learning/quality.rs")
        };

        assert_eq!(
            values(&signals_for(&inline), ConventionKind::TestPlacement),
            vec![("rs".to_string(), "inline_module".to_string())]
        );
    }

    #[test]
    fn context_lines_mentioning_tests_do_not_count_as_adding_them() {
        let untouched = FileChange {
            diff: Some("@@ -1 +1,2 @@\n #[cfg(test)]\n+let value = 1;\n".to_string()),
            ..change("src/workers/learning/quality.rs")
        };

        assert!(
            values(&signals_for(&untouched), ConventionKind::TestPlacement).is_empty(),
            "a test module that was already there is not evidence this run placed one"
        );
    }

    #[test]
    fn a_root_level_file_is_scoped_to_the_repository_root() {
        let signals = signals_for(&change("build_script.sh"));
        assert_eq!(
            values(&signals, ConventionKind::DirectoryContent),
            vec![(".".to_string(), "sh".to_string())]
        );
    }

    #[test]
    fn reading_the_same_change_twice_yields_the_same_signals() {
        let changes = vec![
            change("src/db/knowledge.rs"),
            change("tests/promotion_test.rs"),
        ];
        assert_eq!(
            signals(&changes),
            signals(&changes),
            "signal extraction must be deterministic"
        );
    }
}

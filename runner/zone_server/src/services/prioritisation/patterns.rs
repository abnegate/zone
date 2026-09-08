//! The path vocabulary that decides a blast radius, as configuration.
//!
//! A pattern matches a whole path segment, never a substring, so `core` does
//! not fire on `scoreboard` and `ci` does not fire on `social`. Segments are
//! taken by splitting on the separators that appear in both paths and symbol
//! names, which lets a symbol like `billing_service.charge` be classified with
//! the same vocabulary as a file path.
//!
//! `Default` is a starting point for a repository nobody has configured yet,
//! not a description of this one. Replacing the lists replaces the behaviour.

use super::blast_radius::BlastRadius;

const SEPARATORS: [char; 5] = ['/', '\\', '.', '_', '-'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPatterns {
    pub critical: Vec<String>,
    pub infrastructure: Vec<String>,
    pub cosmetic: Vec<String>,
    pub test: Vec<String>,
    pub core: Vec<String>,
}

impl PathPatterns {
    pub fn empty() -> Self {
        Self {
            critical: Vec::new(),
            infrastructure: Vec::new(),
            cosmetic: Vec::new(),
            test: Vec::new(),
            core: Vec::new(),
        }
    }

    pub fn tier(&self, radius: BlastRadius) -> &[String] {
        match radius {
            BlastRadius::Critical => &self.critical,
            BlastRadius::Infrastructure => &self.infrastructure,
            BlastRadius::Cosmetic => &self.cosmetic,
            BlastRadius::Test => &self.test,
            BlastRadius::Core => &self.core,
            BlastRadius::Peripheral => &[],
        }
    }

    pub fn matches(&self, radius: BlastRadius, signal: &str) -> bool {
        let patterns = self.tier(radius);
        if patterns.is_empty() {
            return false;
        }
        segments(signal).any(|segment| {
            patterns
                .iter()
                .any(|pattern| segment.eq_ignore_ascii_case(pattern))
        })
    }
}

fn segments(signal: &str) -> impl Iterator<Item = &str> {
    signal
        .split(SEPARATORS)
        .filter(|segment| !segment.is_empty())
}

fn owned(patterns: &[&str]) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| (*pattern).to_string())
        .collect()
}

impl Default for PathPatterns {
    fn default() -> Self {
        Self {
            critical: owned(&[
                "auth",
                "authentication",
                "authorisation",
                "authorization",
                "billing",
                "credential",
                "credentials",
                "crypto",
                "encryption",
                "password",
                "payment",
                "payments",
                "permission",
                "permissions",
                "secret",
                "secrets",
                "security",
                "session",
                "token",
            ]),
            infrastructure: owned(&[
                "ansible",
                "cd",
                "ci",
                "compose",
                "deploy",
                "deployment",
                "docker",
                "dockerfile",
                "helm",
                "infra",
                "infrastructure",
                "k8s",
                "kubernetes",
                "migration",
                "migrations",
                "nginx",
                "pipeline",
                "systemd",
                "terraform",
                "workflows",
            ]),
            cosmetic: owned(&[
                "changelog",
                "css",
                "doc",
                "docs",
                "editorconfig",
                "gif",
                "gitignore",
                "ico",
                "jpeg",
                "jpg",
                "licence",
                "license",
                "markdown",
                "md",
                "png",
                "readme",
                "sass",
                "scss",
                "svg",
                "txt",
            ]),
            test: owned(&[
                "bench",
                "benchmark",
                "benchmarks",
                "benches",
                "e2e",
                "fixture",
                "fixtures",
                "mock",
                "mocks",
                "spec",
                "specs",
                "test",
                "testdata",
                "testing",
                "tests",
            ]),
            core: owned(&[
                "api",
                "controller",
                "controllers",
                "core",
                "database",
                "db",
                "domain",
                "engine",
                "handler",
                "handlers",
                "model",
                "models",
                "queue",
                "repository",
                "route",
                "router",
                "routes",
                "scheduler",
                "schema",
                "server",
                "service",
                "services",
                "store",
                "worker",
                "workers",
            ]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PathPatterns;
    use crate::services::prioritisation::blast_radius::BlastRadius;

    #[test]
    fn matching_is_case_insensitive() {
        let patterns = PathPatterns::default();
        assert!(patterns.matches(BlastRadius::Critical, "src/AUTH/Login.rs"));
    }

    #[test]
    fn matching_is_by_segment_not_substring() {
        let patterns = PathPatterns::default();
        assert!(patterns.matches(BlastRadius::Core, "src/api/routes.rs"));
        assert!(!patterns.matches(BlastRadius::Core, "src/scoreboard/render.rs"));
    }

    #[test]
    fn symbol_separators_split_the_same_way_as_paths() {
        let patterns = PathPatterns::default();
        assert!(patterns.matches(BlastRadius::Critical, "billing_service.charge"));
    }

    #[test]
    fn peripheral_has_no_vocabulary_of_its_own() {
        let patterns = PathPatterns::default();
        assert!(patterns.tier(BlastRadius::Peripheral).is_empty());
        assert!(!patterns.matches(BlastRadius::Peripheral, "anything/at/all.rs"));
    }

    #[test]
    fn empty_patterns_match_nothing() {
        let patterns = PathPatterns::empty();
        for radius in [
            BlastRadius::Critical,
            BlastRadius::Infrastructure,
            BlastRadius::Cosmetic,
            BlastRadius::Test,
            BlastRadius::Core,
        ] {
            assert!(
                !patterns.matches(radius, "src/auth/login.rs"),
                "{radius:?} must not match once its vocabulary is cleared"
            );
        }
    }
}

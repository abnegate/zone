//! Auto-detection of a checked-out project's evaluation tooling.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::category::EvalCategory;
use super::lookup::ProgramLookup;

/// How far below the workspace root a manifest is still considered part of it.
pub const MAX_SCAN_DEPTH: usize = 3;

/// Upper bound on distinct projects, so a monorepo cannot fan out without limit.
pub const MAX_PROJECTS: usize = 8;

pub const ROOT_SCOPE: &str = ".";

const CARGO_MANIFEST: &str = "Cargo.toml";
const PACKAGE_MANIFEST: &str = "package.json";

const IGNORED_DIRECTORIES: [&str; 10] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    "vendor",
    "venv",
    "__pycache__",
    "tmp",
    "out",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Cargo,
    JavaScript,
}

/// Which parser turns a tool's output into counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    CargoTest,
    CargoDiagnostics,
    CargoFormat,
    CargoCoverage,
    JavaScriptTest,
    JavaScriptDiagnostics,
    JavaScriptCoverage,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Bun,
    Pnpm,
    Yarn,
    Npm,
}

impl PackageManager {
    pub fn program(self) -> &'static str {
        match self {
            PackageManager::Bun => "bun",
            PackageManager::Pnpm => "pnpm",
            PackageManager::Yarn => "yarn",
            PackageManager::Npm => "npm",
        }
    }

    fn from_declaration(declaration: &str) -> Option<Self> {
        let name = declaration.split('@').next().unwrap_or_default();
        match name {
            "bun" => Some(PackageManager::Bun),
            "pnpm" => Some(PackageManager::Pnpm),
            "yarn" => Some(PackageManager::Yarn),
            "npm" => Some(PackageManager::Npm),
            _ => None,
        }
    }

    fn from_lockfile(directory: &Path) -> Option<Self> {
        const LOCKFILES: [(&str, PackageManager); 5] = [
            ("bun.lock", PackageManager::Bun),
            ("bun.lockb", PackageManager::Bun),
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            ("package-lock.json", PackageManager::Npm),
        ];
        LOCKFILES
            .iter()
            .find(|(name, _)| directory.join(name).is_file())
            .map(|(_, manager)| *manager)
    }

    fn detect(directory: &Path, manifest: &Value) -> Self {
        manifest
            .get("packageManager")
            .and_then(Value::as_str)
            .and_then(Self::from_declaration)
            .or_else(|| Self::from_lockfile(directory))
            .unwrap_or(PackageManager::Npm)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedTool {
    pub ecosystem: Ecosystem,
    pub category: EvalCategory,
    pub name: String,
    pub scope: String,
    pub directory: PathBuf,
    pub program: String,
    pub arguments: Vec<String>,
    pub format: OutputFormat,
}

impl DetectedTool {
    /// Stable key used to pair a before snapshot with its after snapshot.
    pub fn identity(&self) -> String {
        if self.scope == ROOT_SCOPE {
            self.name.clone()
        } else {
            format!("{} @ {}", self.name, self.scope)
        }
    }

    fn new(
        ecosystem: Ecosystem,
        category: EvalCategory,
        name: impl Into<String>,
        project: &Project,
        program: impl Into<String>,
        arguments: &[&str],
        format: OutputFormat,
    ) -> Self {
        Self {
            ecosystem,
            category,
            name: name.into(),
            scope: project.scope.clone(),
            directory: project.directory.clone(),
            program: program.into(),
            arguments: arguments.iter().map(|a| (*a).to_string()).collect(),
            format,
        }
    }
}

/// A manifest root found under the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub ecosystem: Ecosystem,
    pub directory: PathBuf,
    pub scope: String,
}

/// Find every evaluation tool that can run against `root`.
///
/// Returns an empty list, never an error, for a directory with no recognisable
/// tooling.
pub fn detect(root: &Path, lookup: &dyn ProgramLookup) -> Vec<DetectedTool> {
    projects(root)
        .iter()
        .flat_map(|project| match project.ecosystem {
            Ecosystem::Cargo => cargo_tools(project, lookup),
            Ecosystem::JavaScript => javascript_tools(project, lookup),
        })
        .collect()
}

/// Walk the tree for manifest roots, breadth first, so the shallowest manifest
/// of an ecosystem shadows the nested ones it already covers.
pub fn projects(root: &Path) -> Vec<Project> {
    #[derive(Clone, Copy)]
    struct Seeking {
        cargo: bool,
        javascript: bool,
    }

    let mut found = Vec::new();
    let mut queue = VecDeque::from([(
        root.to_path_buf(),
        0usize,
        Seeking {
            cargo: true,
            javascript: true,
        },
    )]);

    while let Some((directory, depth, seeking)) = queue.pop_front() {
        if found.len() >= MAX_PROJECTS {
            break;
        }
        let mut below = seeking;

        if seeking.cargo && directory.join(CARGO_MANIFEST).is_file() {
            found.push(project(root, &directory, Ecosystem::Cargo));
            below.cargo = false;
        }
        if seeking.javascript && directory.join(PACKAGE_MANIFEST).is_file() {
            found.push(project(root, &directory, Ecosystem::JavaScript));
            below.javascript = false;
        }

        if depth >= MAX_SCAN_DEPTH || (!below.cargo && !below.javascript) {
            continue;
        }
        for child in subdirectories(&directory) {
            queue.push_back((child, depth + 1, below));
        }
    }

    found
}

fn project(root: &Path, directory: &Path, ecosystem: Ecosystem) -> Project {
    let scope = directory
        .strip_prefix(root)
        .ok()
        .map(|relative| relative.to_string_lossy().to_string())
        .filter(|relative| !relative.is_empty())
        .unwrap_or_else(|| ROOT_SCOPE.to_string());
    Project {
        ecosystem,
        directory: directory.to_path_buf(),
        scope,
    }
}

fn subdirectories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && !IGNORED_DIRECTORIES.contains(&name.as_ref())
        })
        .map(|entry| entry.path())
        .collect();
    children.sort();
    children
}

fn cargo_tools(project: &Project, lookup: &dyn ProgramLookup) -> Vec<DetectedTool> {
    if !lookup.is_available("cargo") {
        return Vec::new();
    }
    let build = |category, name, arguments: &[&str], format| {
        DetectedTool::new(
            Ecosystem::Cargo,
            category,
            name,
            project,
            "cargo",
            arguments,
            format,
        )
    };

    let mut tools = vec![build(
        EvalCategory::Test,
        "cargo test",
        &["test", "--workspace", "--no-fail-fast"],
        OutputFormat::CargoTest,
    )];

    if lookup.is_available("cargo-clippy") {
        tools.push(build(
            EvalCategory::Lint,
            "cargo clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--message-format",
                "json",
            ],
            OutputFormat::CargoDiagnostics,
        ));
    }
    if lookup.is_available("rustfmt") {
        tools.push(build(
            EvalCategory::Lint,
            "cargo fmt",
            &["fmt", "--all", "--check"],
            OutputFormat::CargoFormat,
        ));
    }
    tools.push(build(
        EvalCategory::Typecheck,
        "cargo check",
        &[
            "check",
            "--workspace",
            "--all-targets",
            "--message-format",
            "json",
        ],
        OutputFormat::CargoDiagnostics,
    ));
    if lookup.is_available("cargo-llvm-cov") {
        tools.push(build(
            EvalCategory::Coverage,
            "cargo llvm-cov",
            &["llvm-cov", "--workspace", "--summary-only", "--json"],
            OutputFormat::CargoCoverage,
        ));
    }

    tools
}

const JAVASCRIPT_SCRIPTS: [(&str, EvalCategory, OutputFormat); 6] = [
    ("test", EvalCategory::Test, OutputFormat::JavaScriptTest),
    (
        "test:coverage",
        EvalCategory::Coverage,
        OutputFormat::JavaScriptCoverage,
    ),
    (
        "lint",
        EvalCategory::Lint,
        OutputFormat::JavaScriptDiagnostics,
    ),
    (
        "format:check",
        EvalCategory::Lint,
        OutputFormat::JavaScriptDiagnostics,
    ),
    (
        "typecheck",
        EvalCategory::Typecheck,
        OutputFormat::JavaScriptDiagnostics,
    ),
    (
        "build",
        EvalCategory::Build,
        OutputFormat::JavaScriptDiagnostics,
    ),
];

fn javascript_tools(project: &Project, lookup: &dyn ProgramLookup) -> Vec<DetectedTool> {
    let Ok(contents) = std::fs::read_to_string(project.directory.join(PACKAGE_MANIFEST)) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&contents) else {
        return Vec::new();
    };
    let manager = PackageManager::detect(&project.directory, &manifest);
    if !lookup.is_available(manager.program()) {
        return Vec::new();
    }
    let Some(scripts) = manifest.get("scripts").and_then(Value::as_object) else {
        return Vec::new();
    };

    JAVASCRIPT_SCRIPTS
        .iter()
        .filter(|(script, _, _)| scripts.contains_key(*script))
        .map(|(script, category, format)| {
            DetectedTool::new(
                Ecosystem::JavaScript,
                *category,
                format!("{} run {}", manager.program(), script),
                project,
                manager.program(),
                &["run", script],
                *format,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::evaluation::lookup::{EveryProgram, NamedPrograms, PathLookup};
    use std::fs;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the zone repository root resolves from the crate manifest")
    }

    fn named(tools: &[DetectedTool]) -> Vec<String> {
        tools.iter().map(DetectedTool::identity).collect()
    }

    fn find<'a>(tools: &'a [DetectedTool], identity: &str) -> &'a DetectedTool {
        tools
            .iter()
            .find(|tool| tool.identity() == identity)
            .unwrap_or_else(|| panic!("expected to detect {identity}, got {:?}", named(tools)))
    }

    #[test]
    fn detects_this_repositorys_own_cargo_and_bun_tooling() {
        let tools = detect(&repository_root(), &EveryProgram);
        let identities = named(&tools);

        for expected in [
            "bun run test",
            "bun run lint",
            "bun run typecheck",
            "bun run format:check",
            "bun run build",
            "bun run test:coverage",
            "cargo test @ runner",
            "cargo clippy @ runner",
            "cargo fmt @ runner",
            "cargo check @ runner",
        ] {
            assert!(
                identities.iter().any(|identity| identity == expected),
                "expected to detect {expected}, got {identities:?}"
            );
        }

        let cargo_test = find(&tools, "cargo test @ runner");
        assert_eq!(cargo_test.directory, repository_root().join("runner"));
        assert_eq!(cargo_test.ecosystem, Ecosystem::Cargo);
        assert_eq!(cargo_test.program, "cargo");
        assert_eq!(cargo_test.format, OutputFormat::CargoTest);

        let bun_test = find(&tools, "bun run test");
        assert_eq!(bun_test.directory, repository_root());
        assert_eq!(bun_test.arguments, vec!["run", "test"]);
        assert_eq!(bun_test.category, EvalCategory::Test);
    }

    #[test]
    fn shadows_nested_manifests_that_a_workspace_root_already_covers() {
        let found = projects(&repository_root());
        let scopes: Vec<&str> = found.iter().map(|p| p.scope.as_str()).collect();

        assert!(
            scopes.contains(&ROOT_SCOPE),
            "the root package.json is the javascript project, got {scopes:?}"
        );
        assert!(scopes.contains(&"runner"), "got {scopes:?}");
        assert!(
            !scopes.iter().any(|scope| scope.starts_with("runner/")),
            "the cargo workspace at runner/ already covers its members, got {scopes:?}"
        );
        assert!(
            !scopes.iter().any(|scope| scope.starts_with("packages/")),
            "the bun workspace root already covers packages/*, got {scopes:?}"
        );
    }

    #[test]
    fn detects_a_bare_cargo_project() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname = \"widget\"\n",
        )
        .expect("manifest written");

        let tools = detect(directory.path(), &EveryProgram);
        let identities = named(&tools);

        assert_eq!(
            identities,
            vec![
                "cargo test",
                "cargo clippy",
                "cargo fmt",
                "cargo check",
                "cargo llvm-cov"
            ]
        );
        assert!(tools.iter().all(|tool| tool.scope == ROOT_SCOPE));
    }

    #[test]
    fn omits_cargo_tools_whose_program_is_not_installed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("Cargo.toml"), "[package]\n").expect("manifest written");

        let tools = detect(directory.path(), &NamedPrograms::new(["cargo"]));

        assert_eq!(named(&tools), vec!["cargo test", "cargo check"]);
    }

    #[test]
    fn detects_nothing_when_cargo_itself_is_missing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("Cargo.toml"), "[package]\n").expect("manifest written");

        assert!(detect(directory.path(), &NamedPrograms::default()).is_empty());
    }

    #[test]
    fn detects_bun_scripts_from_a_lockfile() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"bun test","lint":"biome lint src","typecheck":"tsc --noEmit"}}"#,
        )
        .expect("manifest written");
        fs::write(directory.path().join("bun.lock"), "").expect("lockfile written");

        let tools = detect(directory.path(), &EveryProgram);

        assert_eq!(
            named(&tools),
            vec!["bun run test", "bun run lint", "bun run typecheck"]
        );
        assert_eq!(
            find(&tools, "bun run typecheck").category,
            EvalCategory::Typecheck
        );
        assert_eq!(
            find(&tools, "bun run test").format,
            OutputFormat::JavaScriptTest
        );
    }

    #[test]
    fn prefers_the_declared_package_manager_over_the_lockfile() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"packageManager":"pnpm@9.0.0","scripts":{"test":"vitest run"}}"#,
        )
        .expect("manifest written");
        fs::write(directory.path().join("bun.lock"), "").expect("lockfile written");

        let tools = detect(directory.path(), &EveryProgram);

        assert_eq!(named(&tools), vec!["pnpm run test"]);
        assert_eq!(tools[0].program, "pnpm");
    }

    #[test]
    fn recognises_every_supported_lockfile() {
        for (lockfile, program) in [
            ("bun.lockb", "bun"),
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("package-lock.json", "npm"),
        ] {
            let directory = tempfile::tempdir().expect("temporary directory");
            fs::write(
                directory.path().join("package.json"),
                r#"{"scripts":{"test":"jest"}}"#,
            )
            .expect("manifest written");
            fs::write(directory.path().join(lockfile), "").expect("lockfile written");

            let tools = detect(directory.path(), &EveryProgram);
            assert_eq!(tools[0].program, program, "{lockfile} implies {program}");
        }
    }

    #[test]
    fn falls_back_to_npm_without_a_lockfile() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"jest"}}"#,
        )
        .expect("manifest written");

        assert_eq!(detect(directory.path(), &EveryProgram)[0].program, "npm");
    }

    #[test]
    fn yields_nothing_for_a_project_with_no_tooling() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("README.md"), "# nothing to run\n").expect("file written");
        fs::create_dir(directory.path().join("src")).expect("directory created");

        assert!(
            detect(directory.path(), &EveryProgram).is_empty(),
            "an unrecognised project yields an empty result, not an error"
        );
    }

    #[test]
    fn yields_nothing_for_an_empty_directory() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(detect(directory.path(), &EveryProgram).is_empty());
        assert!(detect(directory.path(), &PathLookup).is_empty());
    }

    #[test]
    fn yields_nothing_for_a_missing_directory() {
        let missing = Path::new("/zone/definitely/not/a/directory");
        assert!(detect(missing, &EveryProgram).is_empty());
    }

    #[test]
    fn ignores_a_package_manifest_with_no_scripts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"library","version":"1.0.0"}"#,
        )
        .expect("manifest written");

        assert!(detect(directory.path(), &EveryProgram).is_empty());
    }

    #[test]
    fn ignores_a_package_manifest_that_is_not_valid_json() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("package.json"), "{ not json").expect("manifest written");

        assert!(detect(directory.path(), &EveryProgram).is_empty());
    }

    #[test]
    fn ignores_unrecognised_scripts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"dev":"vite","release":"semantic-release","test":"bun test"}}"#,
        )
        .expect("manifest written");

        assert_eq!(
            named(&detect(directory.path(), &EveryProgram)),
            vec!["npm run test"]
        );
    }

    #[test]
    fn never_descends_into_dependency_or_output_directories() {
        let directory = tempfile::tempdir().expect("temporary directory");
        for ignored in ["node_modules", "target", ".git"] {
            let nested = directory.path().join(ignored).join("inner");
            fs::create_dir_all(&nested).expect("directory created");
            fs::write(nested.join("Cargo.toml"), "[package]\n").expect("manifest written");
        }

        assert!(detect(directory.path(), &EveryProgram).is_empty());
    }

    #[test]
    fn finds_a_manifest_nested_below_the_root() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let nested = directory.path().join("services").join("api");
        fs::create_dir_all(&nested).expect("directory created");
        fs::write(nested.join("Cargo.toml"), "[package]\n").expect("manifest written");

        let tools = detect(directory.path(), &NamedPrograms::new(["cargo"]));

        assert_eq!(
            named(&tools),
            vec!["cargo test @ services/api", "cargo check @ services/api"]
        );
    }

    #[test]
    fn stops_looking_below_the_scan_depth() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let deep = directory.path().join("a").join("b").join("c").join("d");
        fs::create_dir_all(&deep).expect("directory created");
        fs::write(deep.join("Cargo.toml"), "[package]\n").expect("manifest written");

        assert!(detect(directory.path(), &EveryProgram).is_empty());
    }

    #[test]
    fn caps_the_number_of_projects_it_will_evaluate() {
        let directory = tempfile::tempdir().expect("temporary directory");
        for index in 0..(MAX_PROJECTS + 5) {
            let nested = directory.path().join(format!("crate{index:02}"));
            fs::create_dir_all(&nested).expect("directory created");
            fs::write(nested.join("Cargo.toml"), "[package]\n").expect("manifest written");
        }

        assert_eq!(projects(directory.path()).len(), MAX_PROJECTS);
    }

    #[test]
    fn identities_disambiguate_the_same_tool_in_different_projects() {
        let directory = tempfile::tempdir().expect("temporary directory");
        for name in ["alpha", "beta"] {
            let nested = directory.path().join(name);
            fs::create_dir_all(&nested).expect("directory created");
            fs::write(nested.join("Cargo.toml"), "[package]\n").expect("manifest written");
        }

        let identities = named(&detect(directory.path(), &NamedPrograms::new(["cargo"])));

        assert_eq!(
            identities,
            vec![
                "cargo test @ alpha",
                "cargo check @ alpha",
                "cargo test @ beta",
                "cargo check @ beta"
            ]
        );
    }

    #[test]
    fn detects_both_ecosystems_side_by_side() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(
            directory.path().join("package.json"),
            r#"{"packageManager":"bun@1.4.0","scripts":{"test":"bun test"}}"#,
        )
        .expect("manifest written");
        let rust = directory.path().join("runner");
        fs::create_dir(&rust).expect("directory created");
        fs::write(rust.join("Cargo.toml"), "[workspace]\n").expect("manifest written");

        let tools = detect(directory.path(), &NamedPrograms::new(["cargo", "bun"]));

        assert_eq!(
            named(&tools),
            vec![
                "bun run test",
                "cargo test @ runner",
                "cargo check @ runner"
            ]
        );
        assert_eq!(tools[0].ecosystem, Ecosystem::JavaScript);
        assert_eq!(tools[1].ecosystem, Ecosystem::Cargo);
    }

    #[test]
    fn parses_package_manager_declarations() {
        assert_eq!(
            PackageManager::from_declaration("bun@1.4.0"),
            Some(PackageManager::Bun)
        );
        assert_eq!(
            PackageManager::from_declaration("yarn"),
            Some(PackageManager::Yarn)
        );
        assert_eq!(PackageManager::from_declaration("cargo@1"), None);
    }
}

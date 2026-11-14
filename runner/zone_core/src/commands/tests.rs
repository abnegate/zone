use std::collections::BTreeSet;

use abnegate_agent::ToolContext;
use abnegate_agent::ToolError;
use abnegate_agent::ToolRegistry;
use abnegate_agent::ToolResult;
use abnegate_agent::tool::WaitFor;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;

use super::*;

const RUN_COMMAND: &str = "run_command";

const REFUSED: &str = "not in the allowed list";

/// An argument `run_command` refuses after its allowlist check and before
/// it spawns anything.
const NEVER_SPAWNED: &str = "never;spawned";

const DEFAULT_TOOLS: [&str; 7] = [
    "apply_patch",
    "list_files",
    "read_file",
    "run_command",
    "search_code",
    "tail_job",
    "write_file",
];

fn names(registry: &ToolRegistry) -> BTreeSet<&str> {
    registry.names().into_iter().collect()
}

fn failure(outcome: Result<ToolResult, ToolError>) -> String {
    match outcome {
        Ok(result) => result.error.unwrap_or_default(),
        Err(error) => error.to_string(),
    }
}

#[test]
fn zone_commands_are_the_crate_defaults_and_eight_more() {
    assert_eq!(
        allowed(),
        [
            "cargo", "rustc", "npm", "npx", "yarn", "pnpm", "make", "cmake", "gradle", "mvn",
            "maven", "go", "pip", "pip3", "poetry", "uv", "gem", "bundle", "rake", "dotnet",
            "msbuild", "git", "gh", "hub", "ls", "cat", "head", "tail", "grep", "find", "wc",
            "sort", "uniq", "diff", "tree", "file", "stat", "pwd", "which", "whereis", "pytest",
            "jest", "mocha", "rspec", "phpunit", "echo", "printf", "date", "true", "false", "test",
            "curl", "wget", "jq", "yq", "node", "deno", "bun", "python", "python3", "ruby", "env",
            "docker",
        ]
    );
}

#[tokio::test]
async fn every_extra_program_is_allowed() {
    let directory = TempDir::new().unwrap();
    let context = ToolContext::default().within(directory.path());
    let registry = registry(Shell::Excluded);

    for program in EXTRA_COMMANDS {
        let error = failure(
            registry
                .execute(
                    RUN_COMMAND,
                    json!({"command": program, "args": [NEVER_SPAWNED], "reason": "pin"}),
                    &context,
                )
                .await,
        );
        assert!(!error.contains(REFUSED), "{program}: {error}");
        assert!(error.contains("dangerous pattern"), "{program}: {error}");
    }
}

#[tokio::test]
async fn an_unlisted_program_is_refused_in_the_crates_words() {
    let directory = TempDir::new().unwrap();
    let context = ToolContext::default().within(directory.path());

    let error = registry(Shell::Excluded)
        .execute(
            RUN_COMMAND,
            json!({"command": "perl", "args": ["-e", "1"], "reason": "pin"}),
            &context,
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            &error,
            ToolError::Execution(message) if message
                == "Command 'perl' is not in the allowed list. Name a program, not a path: cargo, npm, git, etc."
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_zero_second_timeout_is_raised_to_one() {
    let directory = TempDir::new().unwrap();
    let context = ToolContext::default().within(directory.path());

    let result = registry(Shell::Excluded)
        .execute(
            RUN_COMMAND,
            json!({"command": "true", "timeout_secs": 0, "reason": "pin"}),
            &context,
        )
        .await
        .unwrap();

    assert!(result.success, "{result:?}");
}

#[test]
fn the_registry_holds_zones_tools_without_wait_for() {
    let excluded = registry(Shell::Excluded);
    assert_eq!(names(&excluded), BTreeSet::from(DEFAULT_TOOLS));

    let included = registry(Shell::Included);
    let mut expected = BTreeSet::from(DEFAULT_TOOLS);
    expected.insert("run_shell");
    assert_eq!(names(&included), expected);
}

#[test]
fn a_registry_without_wait_for_serves_unwaited_tools() {
    for shell in [Shell::Included, Shell::Excluded] {
        let served = registry(shell);
        for tool in registry(shell).into_tools() {
            let expected = tool.unwaited().unwrap_or_else(|| tool.clone());
            let actual = served.get(tool.name()).expect("a registered tool");
            assert_eq!(
                actual.parameters_schema(),
                expected.parameters_schema(),
                "{}",
                tool.name()
            );
        }

        let background = &served
            .get(RUN_COMMAND)
            .expect("run_command")
            .parameters_schema()["properties"]["background"]["description"];
        assert!(
            background
                .as_str()
                .is_some_and(|text| text.contains(WaitFor::Withheld.condition())),
            "{background}"
        );
    }
}

#[test]
fn the_run_and_patch_cards_read_in_the_crates_words() {
    let registry = registry(Shell::Excluded);
    let card = |name: &str, arguments: Value| {
        registry
            .preview(name, &arguments.to_string())
            .expect("a card")
            .text
    };

    assert_eq!(
        card(
            RUN_COMMAND,
            json!({"command": "cargo", "args": ["test"], "cwd": "app"})
        ),
        "In `app`, run `cargo test`."
    );
    assert_eq!(
        card(
            RUN_COMMAND,
            json!({"command": "python3", "args": ["-m", "pytest"]})
        ),
        "In the working directory, run `python3 -m pytest`."
    );
    assert_eq!(
        card(
            "apply_patch",
            json!({
                "path": "src/lib.rs",
                "hunks": [
                    {"old_string": "check()", "new_string": "verify()"},
                    {"old_string": "log()", "new_string": "trace()"},
                ],
            })
        ),
        r#"Edit src/lib.rs: replace "check()" with "verify()"; replace "log()" with "trace()"."#
    );
}

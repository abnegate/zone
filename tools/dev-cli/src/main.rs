use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    ExecutableCommand,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use owo_colors::OwoColorize;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
};
use std::{
    io::{BufRead, BufReader, stdout},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "zone-dev")]
#[command(
    about = "Development CLI tool for Zone - runs format/lint/test/coverage across all projects"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Run without TUI (simple output)
    #[arg(long, short)]
    simple: bool,

    /// Project root directory (defaults to auto-detect)
    #[arg(long, short = 'C')]
    directory: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Format code across all projects
    Format {
        /// Check only, don't modify files
        #[arg(long)]
        check: bool,
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Run linters and static analysis
    Lint {
        /// Auto-fix issues where possible
        #[arg(long)]
        fix: bool,
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Run tests
    Test {
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Run tests with coverage
    Coverage {
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
        /// Open HTML report after completion
        #[arg(long)]
        open: bool,
    },
    /// Run all checks (format, lint, audit, unit tests, e2e tests, lighthouse)
    Check {
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Run Lighthouse performance audits on frontends
    Lighthouse {
        /// Target frontend: manager or all
        #[arg(long, short, value_enum, default_value = "all")]
        target: LighthouseTarget,
    },
    /// Run E2E tests (Playwright)
    E2e {
        /// Only run on specific projects (manager-frontend)
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
        /// Run in headed mode (show browser)
        #[arg(long)]
        headed: bool,
        /// Run in UI mode (interactive)
        #[arg(long)]
        ui: bool,
    },
    /// Run security audit (bun pm audit / cargo audit)
    Audit {
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Database development utilities
    Db {
        #[command(subcommand)]
        command: DbCommands,
    },
    /// Start development environment with hot reload
    Up {
        /// Additional arguments to pass to docker compose
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Stop development environment
    Down {
        /// Additional arguments to pass to docker compose
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show logs from development containers
    Logs {
        /// Additional arguments to pass to docker compose logs
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Restart development environment
    Restart {
        /// Additional arguments to pass to docker compose
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum DbCommands {
    /// Regenerate SQLx offline query cache
    Prepare,
    /// Apply database migrations
    Migrate,
    /// Reset database (drop, recreate, and migrate)
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum LighthouseTarget {
    /// Manager frontend
    Manager,
    /// All frontends
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Project {
    /// Manager frontend (React/TypeScript)
    ManagerFrontend,
    /// Runner (Rust - tool_runner, zone_core, zone_cli)
    Runner,
    /// Server (Rust - zone_server, requires database)
    Server,
}

impl Project {
    fn all() -> Vec<Project> {
        vec![
            Project::ManagerFrontend,
            Project::Runner,
            Project::Server,
        ]
    }

    fn display_name(&self) -> &'static str {
        match self {
            Project::ManagerFrontend => "Manager Frontend",
            Project::Runner => "Runner",
            Project::Server => "Server",
        }
    }

    fn relative_path(&self) -> &'static str {
        match self {
            Project::ManagerFrontend => "manager/frontend",
            Project::Runner => "runner",
            Project::Server => "runner",
        }
    }
}

#[derive(Clone, Debug)]
struct TaskConfig {
    #[allow(dead_code)]
    project: Project,
    name: String,
    command: String,
    args: Vec<String>,
    working_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
enum TaskStatus {
    Pending,
    Running,
    Success,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
struct TaskState {
    config: TaskConfig,
    status: TaskStatus,
    output: Vec<String>,
    start_time: Option<Instant>,
    end_time: Option<Instant>,
    exit_code: Option<i32>,
}

impl TaskState {
    fn new(config: TaskConfig) -> Self {
        Self {
            config,
            status: TaskStatus::Pending,
            output: Vec::new(),
            start_time: None,
            end_time: None,
            exit_code: None,
        }
    }

    fn duration(&self) -> Option<Duration> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end - start),
            (Some(start), None) => Some(Instant::now() - start),
            _ => None,
        }
    }

    fn duration_str(&self) -> String {
        match self.duration() {
            Some(d) => {
                let secs = d.as_secs();
                let millis = d.subsec_millis();
                if secs >= 60 {
                    format!("{}m {}s", secs / 60, secs % 60)
                } else if secs > 0 {
                    format!("{}.{}s", secs, millis / 100)
                } else {
                    format!("{}ms", millis)
                }
            }
            None => "-".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
enum TaskMessage {
    Started(usize),
    Output(usize, String),
    Completed(usize, i32),
    Failed(usize, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FocusedPane {
    Tasks,
    Output,
}

/// Represents an item in the visual task list (either a project header or a task)
#[derive(Clone, Debug)]
enum ListEntry {
    /// Project header (not selectable)
    ProjectHeader(Project),
    /// Task with its index in the tasks Vec
    Task(usize),
}

struct App {
    tasks: Vec<Arc<Mutex<TaskState>>>,
    /// Visual list entries (headers + tasks)
    list_entries: Vec<ListEntry>,
    /// Currently selected index in list_entries
    selected_index: usize,
    should_quit: bool,
    all_done: bool,
    start_time: Instant,
    /// Width of the task pane as a percentage (10-90)
    task_pane_width: u16,
    /// Which pane is currently focused
    focused_pane: FocusedPane,
    /// Scroll offset for the output pane
    output_scroll: usize,
    /// Cached layout areas for mouse hit detection
    tasks_area: Rect,
    output_area: Rect,
}

impl App {
    fn new(tasks: Vec<TaskConfig>) -> Self {
        let task_states: Vec<_> = tasks
            .into_iter()
            .map(|config| Arc::new(Mutex::new(TaskState::new(config))))
            .collect();

        // Group task indices by project
        let mut tasks_by_project: std::collections::BTreeMap<Project, Vec<usize>> =
            std::collections::BTreeMap::new();

        for (idx, task) in task_states.iter().enumerate() {
            let project = task.lock().unwrap().config.project;
            tasks_by_project.entry(project).or_default().push(idx);
        }

        // Build list entries grouped by project
        let mut list_entries = Vec::new();

        for (project, task_indices) in tasks_by_project {
            list_entries.push(ListEntry::ProjectHeader(project));
            for idx in task_indices {
                list_entries.push(ListEntry::Task(idx));
            }
        }

        // Find the first selectable task (skip initial headers)
        let first_task_idx = list_entries
            .iter()
            .position(|e| matches!(e, ListEntry::Task(_)))
            .unwrap_or(0);

        Self {
            tasks: task_states,
            list_entries,
            selected_index: first_task_idx,
            should_quit: false,
            all_done: false,
            start_time: Instant::now(),
            task_pane_width: 40,
            focused_pane: FocusedPane::Tasks,
            output_scroll: 0,
            tasks_area: Rect::default(),
            output_area: Rect::default(),
        }
    }

    /// Get the currently selected task index (if a task is selected)
    fn selected_task(&self) -> Option<usize> {
        match self.list_entries.get(self.selected_index) {
            Some(ListEntry::Task(idx)) => Some(*idx),
            _ => None,
        }
    }

    /// Move selection to the next task (skipping headers)
    fn select_next(&mut self) {
        for i in (self.selected_index + 1)..self.list_entries.len() {
            if matches!(self.list_entries[i], ListEntry::Task(_)) {
                self.selected_index = i;
                self.reset_output_scroll();
                return;
            }
        }
    }

    /// Move selection to the previous task (skipping headers)
    fn select_prev(&mut self) {
        for i in (0..self.selected_index).rev() {
            if matches!(self.list_entries[i], ListEntry::Task(_)) {
                self.selected_index = i;
                self.reset_output_scroll();
                return;
            }
        }
    }

    /// Reset output scroll when selecting a new task
    fn reset_output_scroll(&mut self) {
        self.output_scroll = 0;
    }

    fn completed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                let state = t.lock().unwrap();
                matches!(
                    state.status,
                    TaskStatus::Success | TaskStatus::Failed | TaskStatus::Skipped
                )
            })
            .count()
    }

    fn success_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                let state = t.lock().unwrap();
                state.status == TaskStatus::Success
            })
            .count()
    }

    fn failed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                let state = t.lock().unwrap();
                state.status == TaskStatus::Failed
            })
            .count()
    }

    fn running_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| {
                let state = t.lock().unwrap();
                state.status == TaskStatus::Running
            })
            .count()
    }
}

fn find_project_root(start_dir: Option<PathBuf>) -> Result<PathBuf> {
    let start = start_dir.unwrap_or_else(|| std::env::current_dir().unwrap());
    let mut current = start.as_path();

    loop {
        // Look for markers that indicate project root
        let manager_path = current.join("manager");
        let runner_path = current.join("runner");
        let package_path = current.join("package.json");

        if manager_path.exists() && runner_path.exists() && package_path.exists() {
            return Ok(current.to_path_buf());
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => {
                return Err(anyhow::anyhow!(
                    "Could not find Zone project root. Make sure you're in the project directory."
                ));
            }
        }
    }
}

fn create_format_tasks(root: &PathBuf, projects: &[Project], check: bool) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();
    let mut rust_task_added = false;

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // Check if biome is available
                let biome_config = working_dir.join("biome.json");
                if biome_config.exists() {
                    // biome format without --write checks formatting (exits non-zero if changes needed)
                    // biome format --write applies the formatting changes
                    let args = if check {
                        vec!["biome", "format", "src"]
                    } else {
                        vec!["biome", "format", "--write", "src"]
                    };
                    tasks.push(TaskConfig {
                        project: *project,
                        name: format!("Format {}", project.display_name()),
                        command: "npx".to_string(),
                        args: args.into_iter().map(String::from).collect(),
                        working_dir,
                    });
                } else {
                    // Use prettier if available
                    let package_json = working_dir.join("package.json");
                    if package_json.exists() {
                        if let Ok(content) = std::fs::read_to_string(&package_json) {
                            if content.contains("prettier") {
                                let (cmd, args) = if check {
                                    ("npx", vec!["prettier", "--check", "src"])
                                } else {
                                    ("npx", vec!["prettier", "--write", "src"])
                                };
                                tasks.push(TaskConfig {
                                    project: *project,
                                    name: format!("Format {}", project.display_name()),
                                    command: cmd.to_string(),
                                    args: args.into_iter().map(String::from).collect(),
                                    working_dir,
                                });
                            }
                        }
                    }
                }
            }
            Project::Runner | Project::Server => {
                // Only add format task once for Rust (Runner and Server share same dir)
                if !rust_task_added {
                    rust_task_added = true;
                    let (cmd, args) = if check {
                        ("cargo", vec!["fmt", "--all", "--", "--check"])
                    } else {
                        ("cargo", vec!["fmt", "--all"])
                    };
                    tasks.push(TaskConfig {
                        project: *project,
                        name: "Format Rust".to_string(),
                        command: cmd.to_string(),
                        args: args.into_iter().map(String::from).collect(),
                        working_dir,
                    });
                }
            }
        }
    }

    tasks
}

fn create_lint_tasks(root: &PathBuf, projects: &[Project], fix: bool) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();
    let mut rust_task_added = false;

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // Check for biome
                let biome_config = working_dir.join("biome.json");
                if biome_config.exists() {
                    let args = if fix {
                        vec!["biome", "lint", "--write", "src"]
                    } else {
                        vec!["biome", "lint", "src"]
                    };
                    tasks.push(TaskConfig {
                        project: *project,
                        name: format!("Lint {}", project.display_name()),
                        command: "npx".to_string(),
                        args: args.into_iter().map(String::from).collect(),
                        working_dir: working_dir.clone(),
                    });
                }

                // TypeScript check - use local tsc to avoid npx cache issues
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("TypeCheck {}", project.display_name()),
                    command: format!("{}/node_modules/.bin/tsc", root.display()),
                    args: vec!["--noEmit".to_string()],
                    working_dir,
                });
            }
            Project::Runner | Project::Server => {
                // Only add clippy task once for Rust (Runner and Server share same dir)
                if !rust_task_added {
                    rust_task_added = true;
                    let args = if fix {
                        vec![
                            "clippy",
                            "--all-targets",
                            "--all-features",
                            "--fix",
                            "--allow-dirty",
                            "--",
                            "-D",
                            "warnings",
                        ]
                    } else {
                        vec![
                            "clippy",
                            "--all-targets",
                            "--all-features",
                            "--",
                            "-D",
                            "warnings",
                        ]
                    };
                    tasks.push(TaskConfig {
                        project: *project,
                        name: "Lint Rust".to_string(),
                        command: "cargo".to_string(),
                        args: args.into_iter().map(String::from).collect(),
                        working_dir,
                    });
                }
            }
        }
    }

    tasks
}

fn create_test_tasks(root: &PathBuf, projects: &[Project]) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // Run bun test directly from the frontend directory
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Test {}", project.display_name()),
                    command: "bun".to_string(),
                    args: vec![
                        "test".to_string(),
                        "--max-concurrency=1".to_string(),
                        "src".to_string(),
                    ],
                    working_dir,
                });
            }
            Project::Runner => {
                // Test tool_runner, zone_core, zone_cli (no database needed)
                tasks.push(TaskConfig {
                    project: *project,
                    name: "Test Runner".to_string(),
                    command: "cargo".to_string(),
                    args: vec![
                        "test".to_string(),
                        "-p".to_string(),
                        "tool_runner".to_string(),
                        "-p".to_string(),
                        "zone_core".to_string(),
                        "-p".to_string(),
                        "zone_cli".to_string(),
                    ],
                    working_dir,
                });
            }
            Project::Server => {
                // Test zone_server - start DB containers first, set up test DB, run migrations, then run tests
                // The tests expect postgres://postgres:postgres@localhost:5432/zone_test
                // Stop existing containers and start fresh with test credentials
                let server_dir = root.join("runner/zone_server");
                tasks.push(TaskConfig {
                    project: *project,
                    name: "Test Server".to_string(),
                    command: "bash".to_string(),
                    args: vec![
                        "-c".to_string(),
                        format!(
                            "cd {root} && \
                             docker compose -f docker-compose.yml -f docker-compose.dev.yml stop postgres 2>/dev/null || true && \
                             docker compose -f docker-compose.yml -f docker-compose.dev.yml rm -f postgres 2>/dev/null || true && \
                             POSTGRES_USER=postgres POSTGRES_PASSWORD=postgres POSTGRES_DB=postgres \
                             docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d postgres valkey && \
                             until docker exec postgres pg_isready -U postgres > /dev/null 2>&1; do sleep 1; done && \
                             docker exec postgres psql -U postgres -c \"CREATE DATABASE zone_test;\" 2>/dev/null || true && \
                             cd {server_dir} && DATABASE_URL=postgres://postgres:postgres@localhost:5432/zone_test sqlx migrate run && \
                             cd {working_dir} && DATABASE_URL=postgres://postgres:postgres@localhost:5432/zone_test cargo test -p zone_server",
                            root = root.display(),
                            server_dir = server_dir.display(),
                            working_dir = working_dir.display()
                        ),
                    ],
                    working_dir,
                });
            }
        }
    }

    tasks
}

fn create_e2e_tasks(
    root: &PathBuf,
    projects: &[Project],
    headed: bool,
    ui: bool,
) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();

    for project in projects {
        let project_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // Check if playwright config exists
                let playwright_config = project_dir.join("playwright.config.ts");
                if !playwright_config.exists() {
                    continue;
                }

                // Run bun run test:e2e directly from the frontend directory
                let mut args = vec![
                    "run".to_string(),
                    if ui { "test:e2e:ui" } else { "test:e2e" }.to_string(),
                    "--".to_string(),
                    "--project=chromium".to_string(),
                ];

                // Add headed flag if requested (and not using UI mode)
                if headed && !ui {
                    args.push("--headed".to_string());
                }

                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("E2E {}", project.display_name()),
                    command: "bun".to_string(),
                    args,
                    working_dir: project_dir,
                });
            }
            // E2E tests only apply to frontend projects
            Project::Runner | Project::Server => {}
        }
    }

    tasks
}

fn create_audit_tasks(root: &PathBuf, projects: &[Project]) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();
    let mut rust_task_added = false;

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // bun audit for JavaScript/TypeScript
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Audit {}", project.display_name()),
                    command: "bun".to_string(),
                    args: vec!["audit".to_string()],
                    working_dir,
                });
            }
            Project::Runner | Project::Server => {
                // cargo audit for Rust - only add once (Runner and Server share same dir)
                if !rust_task_added {
                    rust_task_added = true;
                    tasks.push(TaskConfig {
                        project: *project,
                        name: "Audit Rust".to_string(),
                        command: "cargo".to_string(),
                        args: vec!["audit".to_string()],
                        working_dir,
                    });
                }
            }
        }
    }

    tasks
}

fn create_coverage_tasks(root: &PathBuf, projects: &[Project]) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();
    let mut rust_task_added = false;

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::ManagerFrontend => {
                // Run bun test with coverage
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Coverage {}", project.display_name()),
                    command: "bun".to_string(),
                    args: vec![
                        "test".to_string(),
                        "src".to_string(),
                        "--coverage".to_string(),
                    ],
                    working_dir,
                });
            }
            Project::Runner | Project::Server => {
                // Only add coverage task once for Rust (Runner and Server share same dir)
                if !rust_task_added {
                    rust_task_added = true;
                    tasks.push(TaskConfig {
                        project: *project,
                        name: "Coverage Rust".to_string(),
                        command: "cargo".to_string(),
                        args: vec!["llvm-cov".to_string(), "--html".to_string()],
                        working_dir,
                    });
                }
            }
        }
    }

    tasks
}

fn create_lighthouse_tasks(root: &PathBuf, target: LighthouseTarget) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();

    let targets = match target {
        LighthouseTarget::Manager | LighthouseTarget::All => vec![Project::ManagerFrontend],
    };

    for project in targets {
        let working_dir = root.join(project.relative_path());

        // Build then run Lighthouse CI
        // Note: Assumes dependencies are already installed
        tasks.push(TaskConfig {
            project,
            name: format!("Lighthouse {}", project.display_name()),
            command: "bash".to_string(),
            args: vec![
                "-c".to_string(),
                "bun run build && bun run lighthouse".to_string(),
            ],
            working_dir,
        });
    }

    tasks
}

async fn run_task(task_idx: usize, task: Arc<Mutex<TaskState>>, tx: mpsc::Sender<TaskMessage>) {
    let config = {
        let state = task.lock().unwrap();
        state.config.clone()
    };

    // Mark as started
    {
        let mut state = task.lock().unwrap();
        state.status = TaskStatus::Running;
        state.start_time = Some(Instant::now());
    }
    let _ = tx.send(TaskMessage::Started(task_idx)).await;

    // Check if working directory exists
    if !config.working_dir.exists() {
        let _ = tx
            .send(TaskMessage::Output(
                task_idx,
                format!("Working directory not found: {:?}", config.working_dir),
            ))
            .await;
        let mut state = task.lock().unwrap();
        state.status = TaskStatus::Skipped;
        state.end_time = Some(Instant::now());
        return;
    }

    // Check if command exists
    if which::which(&config.command).is_err() {
        let _ = tx
            .send(TaskMessage::Output(
                task_idx,
                format!("Command not found: {}", config.command),
            ))
            .await;
        let mut state = task.lock().unwrap();
        state.status = TaskStatus::Skipped;
        state.end_time = Some(Instant::now());
        return;
    }

    // Run the command
    let mut cmd = Command::new(&config.command);

    // Get current PATH and prepend node_modules/.bin directories
    let path_var = std::env::var("PATH").unwrap_or_default();
    let root_bin = config.working_dir.join("node_modules/.bin");
    let root_root_bin = config
        .working_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("node_modules/.bin"));

    let mut new_path = String::new();
    if root_bin.exists() {
        new_path.push_str(&root_bin.to_string_lossy());
        new_path.push(':');
    }
    if let Some(ref bin) = root_root_bin {
        if bin.exists() {
            new_path.push_str(&bin.to_string_lossy());
            new_path.push(':');
        }
    }
    new_path.push_str(&path_var);

    cmd.args(&config.args)
        .current_dir(&config.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CI", "true")
        .env("PATH", &new_path);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tx
                .send(TaskMessage::Failed(
                    task_idx,
                    format!("Failed to spawn: {}", e),
                ))
                .await;
            let mut state = task.lock().unwrap();
            state.status = TaskStatus::Failed;
            state.end_time = Some(Instant::now());
            return;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Read stdout and stderr in separate threads
    let tx_stdout = tx.clone();
    let stdout_handle = std::thread::spawn(move || {
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_stdout.blocking_send(TaskMessage::Output(task_idx, line));
            }
        }
    });

    let tx_stderr = tx.clone();
    let stderr_handle = std::thread::spawn(move || {
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_stderr.blocking_send(TaskMessage::Output(task_idx, line));
            }
        }
    });

    // Wait for output readers to finish
    stdout_handle.join().ok();
    stderr_handle.join().ok();

    // Wait for the process to complete and get exit status
    let exit_status = match child.wait() {
        Ok(status) => Some(status.code().unwrap_or(-1)),
        Err(_) => None,
    };

    // Update state and drop lock before await
    {
        let mut state = task.lock().unwrap();
        state.end_time = Some(Instant::now());
        match exit_status {
            Some(0) => {
                state.status = TaskStatus::Success;
                state.exit_code = Some(0);
            }
            Some(code) => {
                state.status = TaskStatus::Failed;
                state.exit_code = Some(code);
            }
            None => {
                state.status = TaskStatus::Failed;
            }
        }
    }

    // Send messages after lock is dropped
    match exit_status {
        Some(code) => {
            let _ = tx.send(TaskMessage::Completed(task_idx, code)).await;
        }
        None => {
            let _ = tx
                .send(TaskMessage::Failed(
                    task_idx,
                    "Process terminated".to_string(),
                ))
                .await;
        }
    }
}

fn draw_ui(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Progress
            Constraint::Min(10),   // Task list
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    // Header
    let header_text = format!(" Zone Dev Tools - {} ", Local::now().format("%H:%M:%S"));
    let header = Paragraph::new(header_text)
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title("Zone"));
    frame.render_widget(header, chunks[0]);

    // Progress bar
    let completed = app.completed_count();
    let total = app.tasks.len();
    let progress = if total > 0 {
        completed as f64 / total as f64
    } else {
        0.0
    };

    let elapsed = app.start_time.elapsed();
    let elapsed_str = format!("{}m {:02}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60);

    let progress_label = format!(
        "{}/{} tasks | {} running | {} passed | {} failed | {}",
        completed,
        total,
        app.running_count(),
        app.success_count(),
        app.failed_count(),
        elapsed_str
    );

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(
            Style::default()
                .fg(if app.failed_count() > 0 {
                    Color::Red
                } else if app.all_done {
                    Color::Green
                } else {
                    Color::Blue
                })
                .bg(Color::DarkGray),
        )
        .percent((progress * 100.0) as u16)
        .label(progress_label);
    frame.render_widget(gauge, chunks[1]);

    // Split the main area into task list and output (resizable)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.task_pane_width),
            Constraint::Percentage(100 - app.task_pane_width),
        ])
        .split(chunks[2]);

    // Save areas for mouse hit detection
    app.tasks_area = main_chunks[0];
    app.output_area = main_chunks[1];

    // Task list with project grouping
    let items: Vec<ListItem> = app
        .list_entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            match entry {
                ListEntry::ProjectHeader(project) => {
                    // Project header - styled differently, not selectable
                    let content = Line::from(vec![Span::styled(
                        format!("▸ {}", project.display_name()),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )]);
                    ListItem::new(content)
                }
                ListEntry::Task(task_idx) => {
                    let state = app.tasks[*task_idx].lock().unwrap();
                    let (status_icon, status_color) = match state.status {
                        TaskStatus::Pending => ("○", Color::DarkGray),
                        TaskStatus::Running => ("◐", Color::Yellow),
                        TaskStatus::Success => ("✓", Color::Green),
                        TaskStatus::Failed => ("✗", Color::Red),
                        TaskStatus::Skipped => ("⊘", Color::DarkGray),
                    };

                    let duration = state.duration_str();
                    let is_selected = idx == app.selected_index;

                    let style = if is_selected {
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    // Extract just the task type from the name (remove project prefix)
                    let task_name = state
                        .config
                        .name
                        .split_whitespace()
                        .next()
                        .unwrap_or(&state.config.name);

                    let content = Line::from(vec![
                        Span::raw("  "), // Indent for tasks under project
                        Span::styled(
                            format!("{} ", status_icon),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(
                            format!("{:<16}", task_name),
                            Style::default().fg(if is_selected {
                                Color::White
                            } else {
                                Color::Gray
                            }),
                        ),
                        Span::styled(
                            format!(" {:>8}", duration),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);

                    ListItem::new(content).style(style)
                }
            }
        })
        .collect();

    let tasks_title = if app.focused_pane == FocusedPane::Tasks {
        "Tasks [focused]"
    } else {
        "Tasks"
    };
    let tasks_border_style = if app.focused_pane == FocusedPane::Tasks {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(tasks_title)
                .border_style(tasks_border_style),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, main_chunks[0]);

    // Output panel - calculate available height for scrolling
    let output_area_height = main_chunks[1].height.saturating_sub(2) as usize; // subtract borders

    let (selected_output, total_lines, scroll_offset): (Vec<Line>, usize, usize) =
        if let Some(task_idx) = app.selected_task() {
            let state = app.tasks[task_idx].lock().unwrap();
            let cmd_line = format!("$ {} {}", state.config.command, state.config.args.join(" "));
            let dir_line = format!("Directory: {:?}", state.config.working_dir);

            // Clone output to avoid lifetime issues
            let output_clone: Vec<String> = state.output.clone();
            drop(state); // Explicitly drop the guard

            let mut lines = vec![
                Line::from(Span::styled(cmd_line, Style::default().fg(Color::Cyan))),
                Line::from(Span::styled(dir_line, Style::default().fg(Color::DarkGray))),
                Line::from(""),
            ];

            // Add all output lines with color coding
            for line in &output_clone {
                let color = if line.contains("error")
                    || line.contains("Error")
                    || line.contains("FAILED")
                {
                    Color::Red
                } else if line.contains("warning") || line.contains("Warning") {
                    Color::Yellow
                } else if line.contains("passed") || line.contains("success") || line.contains("ok")
                {
                    Color::Green
                } else {
                    Color::Gray
                };
                lines.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(color),
                )));
            }

            let total = lines.len();
            // Clamp scroll offset to valid range
            let max_scroll = total.saturating_sub(output_area_height);
            let scroll = app.output_scroll.min(max_scroll);

            (lines, total, scroll)
        } else {
            (vec![Line::from("No task selected")], 1, 0)
        };

    // Build output title with scroll info
    let output_title = if app.focused_pane == FocusedPane::Output {
        if total_lines > output_area_height {
            format!(
                "Output [focused] ({}-{}/{})",
                scroll_offset + 1,
                (scroll_offset + output_area_height).min(total_lines),
                total_lines
            )
        } else {
            "Output [focused]".to_string()
        }
    } else if total_lines > output_area_height {
        format!(
            "Output ({}-{}/{})",
            scroll_offset + 1,
            (scroll_offset + output_area_height).min(total_lines),
            total_lines
        )
    } else {
        "Output".to_string()
    };

    let output_border_style = if app.focused_pane == FocusedPane::Output {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let output = Paragraph::new(selected_output)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(output_title)
                .border_style(output_border_style),
        )
        .scroll((scroll_offset as u16, 0))
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(output, main_chunks[1]);

    // Footer with context-sensitive help
    let base_help = "q:quit  Tab:switch pane  []:resize";
    let nav_help = if app.focused_pane == FocusedPane::Tasks {
        "↑↓/jk:select task"
    } else {
        "↑↓/jk:scroll  PgUp/PgDn  g/G:top/bottom"
    };
    let status = if app.all_done {
        if app.failed_count() > 0 {
            "Some tasks failed!"
        } else {
            "All tasks completed!"
        }
    } else {
        "Running..."
    };
    let footer_text = format!(" {} | {} | {} ", base_help, nav_help, status);
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, chunks[3]);
}

async fn run_tui(tasks: Vec<TaskConfig>) -> Result<bool> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(tasks);
    let (tx, mut rx) = mpsc::channel::<TaskMessage>(100);

    // Start running tasks in background
    let tasks_ref: Vec<_> = app.tasks.iter().map(Arc::clone).collect();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut handles = Vec::new();
        for (idx, task) in tasks_ref.into_iter().enumerate() {
            let tx = tx_clone.clone();
            let handle = tokio::spawn(async move {
                run_task(idx, task, tx).await;
            });
            handles.push(handle);
        }
        for handle in handles {
            let _ = handle.await;
        }
    });

    loop {
        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut app))?;

        // Handle messages from tasks
        while let Ok(msg) = rx.try_recv() {
            match msg {
                TaskMessage::Started(_) => {}
                TaskMessage::Output(idx, line) => {
                    if let Some(task) = app.tasks.get(idx) {
                        let mut state = task.lock().unwrap();
                        state.output.push(line);
                    }
                }
                TaskMessage::Completed(idx, code) => {
                    if let Some(task) = app.tasks.get(idx) {
                        let mut state = task.lock().unwrap();
                        state.exit_code = Some(code);
                        state.end_time = Some(Instant::now());
                        state.status = if code == 0 {
                            TaskStatus::Success
                        } else {
                            TaskStatus::Failed
                        };
                    }
                }
                TaskMessage::Failed(idx, msg) => {
                    if let Some(task) = app.tasks.get(idx) {
                        let mut state = task.lock().unwrap();
                        state.output.push(format!("Error: {}", msg));
                        state.status = TaskStatus::Failed;
                        state.end_time = Some(Instant::now());
                    }
                }
            }
        }

        // Check if all done
        app.all_done = app.completed_count() == app.tasks.len();

        // Handle input (keyboard and mouse)
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => {
                                app.should_quit = true;
                            }
                            // Tab switches focus between panes
                            KeyCode::Tab => {
                                app.focused_pane = match app.focused_pane {
                                    FocusedPane::Tasks => FocusedPane::Output,
                                    FocusedPane::Output => FocusedPane::Tasks,
                                };
                            }
                            // Resize pane width with [ and ]
                            KeyCode::Char('[') => {
                                app.task_pane_width = app.task_pane_width.saturating_sub(5).max(15);
                            }
                            KeyCode::Char(']') => {
                                app.task_pane_width = (app.task_pane_width + 5).min(85);
                            }
                            // Up/Down navigation depends on focused pane
                            KeyCode::Up | KeyCode::Char('k') => match app.focused_pane {
                                FocusedPane::Tasks => {
                                    app.select_prev();
                                }
                                FocusedPane::Output => {
                                    app.output_scroll = app.output_scroll.saturating_sub(1);
                                }
                            },
                            KeyCode::Down | KeyCode::Char('j') => match app.focused_pane {
                                FocusedPane::Tasks => {
                                    app.select_next();
                                }
                                FocusedPane::Output => {
                                    app.output_scroll = app.output_scroll.saturating_add(1);
                                }
                            },
                            // Page up/down for faster scrolling in output pane
                            KeyCode::PageUp => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = app.output_scroll.saturating_sub(10);
                                }
                            }
                            KeyCode::PageDown => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = app.output_scroll.saturating_add(10);
                                }
                            }
                            // Home/End for jumping to start/end of output
                            KeyCode::Home => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = 0;
                                }
                            }
                            KeyCode::End => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = usize::MAX; // Will be clamped in draw_ui
                                }
                            }
                            // 'g' to scroll to top, 'G' to scroll to bottom (vim-style)
                            KeyCode::Char('g') => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = 0;
                                }
                            }
                            KeyCode::Char('G') => {
                                if app.focused_pane == FocusedPane::Output {
                                    app.output_scroll = usize::MAX;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    let x = mouse.column;
                    let y = mouse.row;

                    // Check if click is in tasks area
                    let in_tasks = x >= app.tasks_area.x
                        && x < app.tasks_area.x + app.tasks_area.width
                        && y >= app.tasks_area.y
                        && y < app.tasks_area.y + app.tasks_area.height;

                    // Check if click is in output area
                    let in_output = x >= app.output_area.x
                        && x < app.output_area.x + app.output_area.width
                        && y >= app.output_area.y
                        && y < app.output_area.y + app.output_area.height;

                    match mouse.kind {
                        MouseEventKind::Down(_) => {
                            if in_tasks {
                                app.focused_pane = FocusedPane::Tasks;
                                // Calculate which list entry was clicked (accounting for border)
                                let entry_y = y.saturating_sub(app.tasks_area.y + 1);
                                let entry_idx = entry_y as usize;
                                // Only select if it's a valid task entry (not a header)
                                if entry_idx < app.list_entries.len() {
                                    if matches!(app.list_entries[entry_idx], ListEntry::Task(_)) {
                                        app.selected_index = entry_idx;
                                        app.reset_output_scroll();
                                    }
                                }
                            } else if in_output {
                                app.focused_pane = FocusedPane::Output;
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if in_output {
                                app.output_scroll = app.output_scroll.saturating_sub(3);
                            } else if in_tasks {
                                app.select_prev();
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if in_output {
                                app.output_scroll = app.output_scroll.saturating_add(3);
                            } else if in_tasks {
                                app.select_next();
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup
    stdout().execute(DisableMouseCapture)?;
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    Ok(app.failed_count() == 0)
}

async fn run_simple(tasks: Vec<TaskConfig>) -> Result<bool> {
    println!("{} Running {} tasks...\n", "→".cyan().bold(), tasks.len());

    let (tx, mut rx) = mpsc::channel::<TaskMessage>(100);

    let task_states: Vec<_> = tasks
        .into_iter()
        .map(|config| Arc::new(Mutex::new(TaskState::new(config))))
        .collect();

    // Clone for running
    let tasks_for_run: Vec<_> = task_states.iter().map(Arc::clone).collect();
    let tx_clone = tx.clone();

    tokio::spawn(async move {
        let mut handles = Vec::new();
        for (idx, task) in tasks_for_run.into_iter().enumerate() {
            let tx = tx_clone.clone();
            let handle = tokio::spawn(async move {
                run_task(idx, task, tx).await;
            });
            handles.push(handle);
        }
        drop(tx_clone); // Drop after all clones are made so receiver can detect completion
        for handle in handles {
            let _ = handle.await;
        }
    });

    // Track completion
    let mut completed = 0;
    let total = task_states.len();
    let mut success = 0;
    let mut failed = 0;

    // Process messages
    drop(tx); // Drop sender so receiver can detect completion

    while let Some(msg) = rx.recv().await {
        match msg {
            TaskMessage::Started(idx) => {
                let state = task_states[idx].lock().unwrap();
                println!(
                    "{} {} {}",
                    "→".cyan(),
                    state.config.name,
                    "(running)".dimmed()
                );
            }
            TaskMessage::Output(idx, line) => {
                let mut state = task_states[idx].lock().unwrap();
                state.output.push(line);
            }
            TaskMessage::Completed(idx, code) => {
                let mut state = task_states[idx].lock().unwrap();
                state.exit_code = Some(code);
                state.end_time = Some(Instant::now());
                completed += 1;

                if code == 0 {
                    state.status = TaskStatus::Success;
                    success += 1;
                    println!(
                        "{} {} {} {}",
                        "✓".green().bold(),
                        state.config.name,
                        state.duration_str().dimmed(),
                        format!("({}/{})", completed, total).dimmed()
                    );
                } else {
                    state.status = TaskStatus::Failed;
                    failed += 1;
                    println!(
                        "{} {} {} (exit code: {})",
                        "✗".red().bold(),
                        state.config.name,
                        state.duration_str().dimmed(),
                        code
                    );
                    // Print last few lines of output
                    let output_lines: Vec<_> = state.output.iter().rev().take(10).collect();
                    for line in output_lines.into_iter().rev() {
                        println!("  {}", line.dimmed());
                    }
                }
            }
            TaskMessage::Failed(idx, error_msg) => {
                let mut state = task_states[idx].lock().unwrap();
                state.status = TaskStatus::Failed;
                state.end_time = Some(Instant::now());
                completed += 1;
                failed += 1;
                println!(
                    "{} {} - {}",
                    "✗".red().bold(),
                    state.config.name,
                    error_msg.red()
                );
            }
        }

        // Exit once all tasks have reported completion
        if completed == total {
            break;
        }
    }

    println!();
    if failed == 0 {
        println!(
            "{} All {} tasks completed successfully!",
            "✓".green().bold(),
            total
        );
    } else {
        println!(
            "{} {} passed, {} failed",
            "→".cyan(),
            success.to_string().green(),
            failed.to_string().red()
        );
    }

    Ok(failed == 0)
}

fn run_docker_compose(root: &PathBuf, subcommand: &str, extra_args: &[String]) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("compose")
        .arg("-f")
        .arg(root.join("docker-compose.yml"))
        .arg("-f")
        .arg(root.join("docker-compose.dev.yml"))
        .arg(subcommand)
        .args(extra_args)
        .current_dir(root);

    // For 'up', add default flags if not specified
    if subcommand == "up" && !extra_args.iter().any(|a| a == "-d" || a == "--detach") {
        // Run in foreground by default for dev (shows logs)
    }

    let status = cmd.status()?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "docker compose {} failed with exit code: {:?}",
            subcommand,
            status.code()
        ));
    }

    Ok(())
}

async fn run_db_command(root: &PathBuf, command: &DbCommands, _simple: bool) -> Result<()> {
    match command {
        DbCommands::Prepare => {
            println!("{} Regenerating SQLx offline cache...", "→".cyan());

            let zone_server_dir = root.join("runner/zone_server");
            if !zone_server_dir.exists() {
                return Err(anyhow::anyhow!(
                    "zone_server directory not found at {:?}",
                    zone_server_dir
                ));
            }

            // Check if cargo-sqlx is installed
            let check = Command::new("cargo").args(["sqlx", "--version"]).output()?;

            if !check.status.success() {
                println!("{} cargo-sqlx not found, installing...", "→".yellow());
                let install = Command::new("cargo")
                    .args([
                        "install",
                        "sqlx-cli",
                        "--version",
                        "0.8.6",
                        "--locked",
                        "--no-default-features",
                        "--features",
                        "postgres",
                    ])
                    .status()?;

                if !install.success() {
                    return Err(anyhow::anyhow!("Failed to install sqlx-cli"));
                }
            }

            let status = Command::new("cargo")
                .args(["sqlx", "prepare"])
                .current_dir(&zone_server_dir)
                .status()?;

            if !status.success() {
                return Err(anyhow::anyhow!("cargo sqlx prepare failed"));
            }

            println!(
                "{} SQLx offline cache regenerated successfully",
                "✓".green().bold()
            );
            println!(
                "{}",
                "  Don't forget to commit the .sqlx directory!".dimmed()
            );
        }
        DbCommands::Migrate => {
            println!("{} Applying database migrations...", "→".cyan());

            let migration_file = root.join("runner/zone_server/migrations/001_initial_schema.sql");
            if !migration_file.exists() {
                return Err(anyhow::anyhow!(
                    "Migration file not found: {:?}",
                    migration_file
                ));
            }

            let status = Command::new("docker")
                .args([
                    "exec", "-i", "postgres", "psql", "-U", "litellm", "-d", "manager",
                ])
                .stdin(std::fs::File::open(&migration_file)?)
                .status()?;

            if !status.success() {
                return Err(anyhow::anyhow!("Migration failed"));
            }

            println!("{} Migrations applied successfully", "✓".green().bold());
        }
        DbCommands::Reset => {
            println!("{} Resetting database...", "→".cyan());

            // Drop database
            let _ = Command::new("docker")
                .args([
                    "exec",
                    "postgres",
                    "psql",
                    "-U",
                    "litellm",
                    "-c",
                    "DROP DATABASE IF EXISTS manager;",
                ])
                .status();

            // Create database
            let create = Command::new("docker")
                .args([
                    "exec",
                    "postgres",
                    "psql",
                    "-U",
                    "litellm",
                    "-c",
                    "CREATE DATABASE manager;",
                ])
                .status()?;

            if !create.success() {
                return Err(anyhow::anyhow!("Failed to create database"));
            }

            // Apply migrations
            let migration_file = root.join("runner/zone_server/migrations/001_initial_schema.sql");
            let status = Command::new("docker")
                .args([
                    "exec", "-i", "postgres", "psql", "-U", "litellm", "-d", "manager",
                ])
                .stdin(std::fs::File::open(&migration_file)?)
                .status()?;

            if !status.success() {
                return Err(anyhow::anyhow!("Migration failed"));
            }

            println!("{} Database reset complete", "✓".green().bold());
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let root = find_project_root(cli.directory)?;
    println!("{} Project root: {:?}", "→".cyan(), root);

    let get_projects = |project_filter: Option<Vec<Project>>| -> Vec<Project> {
        project_filter.unwrap_or_else(Project::all)
    };

    let tasks = match &cli.command {
        Commands::Format { check, project } => {
            create_format_tasks(&root, &get_projects(project.clone()), *check)
        }
        Commands::Lint { fix, project } => {
            create_lint_tasks(&root, &get_projects(project.clone()), *fix)
        }
        Commands::Test { project } => create_test_tasks(&root, &get_projects(project.clone())),
        Commands::Coverage { project, open: _ } => {
            create_coverage_tasks(&root, &get_projects(project.clone()))
        }
        Commands::Check { project } => {
            let projects = get_projects(project.clone());
            let mut all_tasks = Vec::new();
            all_tasks.extend(create_format_tasks(&root, &projects, true));
            all_tasks.extend(create_lint_tasks(&root, &projects, false));
            all_tasks.extend(create_audit_tasks(&root, &projects));
            all_tasks.extend(create_test_tasks(&root, &projects));
            all_tasks.extend(create_e2e_tasks(&root, &projects, false, false));
            // Only run lighthouse for frontend projects
            let lighthouse_target = match project {
                Some(ps) if ps.contains(&Project::ManagerFrontend) => {
                    Some(LighthouseTarget::Manager)
                }
                Some(_) => None, // Only backend projects selected
                None => Some(LighthouseTarget::All), // No filter = all projects
            };
            if let Some(target) = lighthouse_target {
                all_tasks.extend(create_lighthouse_tasks(&root, target));
            }
            all_tasks
        }
        Commands::Lighthouse { target } => create_lighthouse_tasks(&root, *target),
        Commands::E2e {
            project,
            headed,
            ui,
        } => create_e2e_tasks(&root, &get_projects(project.clone()), *headed, *ui),
        Commands::Audit { project } => create_audit_tasks(&root, &get_projects(project.clone())),
        Commands::Db { command } => {
            return run_db_command(&root, &command, cli.simple).await;
        }
        Commands::Up { args } => {
            println!(
                "{} Starting development environment with hot reload...",
                "→".cyan()
            );
            println!(
                "{}",
                "  Rust server: cargo-watch rebuilds on file changes".dimmed()
            );
            println!("{}", "  Frontend: Vite HMR enabled".dimmed());
            println!();
            return run_docker_compose(&root, "up", args);
        }
        Commands::Down { args } => {
            println!("{} Stopping development environment...", "→".cyan());
            return run_docker_compose(&root, "down", args);
        }
        Commands::Logs { args } => {
            return run_docker_compose(&root, "logs", args);
        }
        Commands::Restart { args } => {
            println!("{} Restarting development environment...", "→".cyan());
            return run_docker_compose(&root, "restart", args);
        }
    };

    if tasks.is_empty() {
        println!("{} No tasks to run", "!".yellow());
        return Ok(());
    }

    let success = if cli.simple {
        run_simple(tasks).await?
    } else {
        run_tui(tasks).await?
    };

    if !success {
        std::process::exit(1);
    }

    Ok(())
}

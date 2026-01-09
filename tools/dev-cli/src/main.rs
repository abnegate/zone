use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use owo_colors::OwoColorize;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{
    io::{stdout, BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "zone-dev")]
#[command(about = "Development CLI tool for Zone - runs format/lint/test/coverage across all projects")]
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
    /// Run all checks (format --check, lint, test)
    Check {
        /// Only run on specific projects
        #[arg(long, short, value_enum)]
        project: Option<Vec<Project>>,
    },
    /// Run Lighthouse performance audits on frontends
    Lighthouse {
        /// Target frontend: installer, manager, or all
        #[arg(long, short, value_enum, default_value = "all")]
        target: LighthouseTarget,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum LighthouseTarget {
    /// Installer frontend
    Installer,
    /// Manager frontend
    Manager,
    /// Both frontends
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Project {
    /// Installer frontend (React/TypeScript)
    InstallerFrontend,
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
            Project::InstallerFrontend,
            Project::ManagerFrontend,
            Project::Runner,
            Project::Server,
        ]
    }

    fn display_name(&self) -> &'static str {
        match self {
            Project::InstallerFrontend => "Installer Frontend",
            Project::ManagerFrontend => "Manager Frontend",
            Project::Runner => "Runner",
            Project::Server => "Server",
        }
    }

    fn relative_path(&self) -> &'static str {
        match self {
            Project::InstallerFrontend => "installer/frontend",
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

struct App {
    tasks: Vec<Arc<Mutex<TaskState>>>,
    selected_task: usize,
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

        Self {
            tasks: task_states,
            selected_task: 0,
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
        let installer_path = current.join("installer");
        let manager_path = current.join("manager");
        let runner_path = current.join("runner");

        if installer_path.exists() && manager_path.exists() && runner_path.exists() {
            return Ok(current.to_path_buf());
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => {
                return Err(anyhow::anyhow!(
                    "Could not find Zone project root. Make sure you're in the project directory."
                ))
            }
        }
    }
}

fn create_format_tasks(root: &PathBuf, projects: &[Project], check: bool) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::InstallerFrontend | Project::ManagerFrontend => {
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
                // Only add format task once for Rust (both share same dir)
                if *project == Project::Runner {
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

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::InstallerFrontend | Project::ManagerFrontend => {
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

                // TypeScript check
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("TypeCheck {}", project.display_name()),
                    command: "npx".to_string(),
                    args: vec!["tsc".to_string(), "--noEmit".to_string()],
                    working_dir,
                });
            }
            Project::Runner | Project::Server => {
                // Only add clippy task once for Rust (both share same dir)
                if *project == Project::Runner {
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
            Project::InstallerFrontend | Project::ManagerFrontend => {
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Test {}", project.display_name()),
                    command: "npm".to_string(),
                    args: vec!["test".to_string(), "--".to_string(), "--watchAll=false".to_string()],
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
                        "-p".to_string(), "tool_runner".to_string(),
                        "-p".to_string(), "zone_core".to_string(),
                        "-p".to_string(), "zone_cli".to_string(),
                    ],
                    working_dir,
                });
            }
            Project::Server => {
                // Test zone_server (requires DATABASE_URL from env or .env)
                tasks.push(TaskConfig {
                    project: *project,
                    name: "Test Server".to_string(),
                    command: "cargo".to_string(),
                    args: vec![
                        "test".to_string(),
                        "-p".to_string(), "zone_server".to_string(),
                    ],
                    working_dir,
                });
            }
        }
    }

    tasks
}

fn create_coverage_tasks(root: &PathBuf, projects: &[Project]) -> Vec<TaskConfig> {
    let mut tasks = Vec::new();

    for project in projects {
        let working_dir = root.join(project.relative_path());

        match project {
            Project::InstallerFrontend | Project::ManagerFrontend => {
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Coverage {}", project.display_name()),
                    command: "npm".to_string(),
                    args: vec!["run".to_string(), "test:coverage".to_string()],
                    working_dir,
                });
            }
            Project::Runner | Project::Server => {
                // Only add coverage task once for Rust
                if *project == Project::Runner {
                    tasks.push(TaskConfig {
                        project: *project,
                        name: "Coverage Rust".to_string(),
                        command: "cargo".to_string(),
                        args: vec![
                            "llvm-cov".to_string(),
                            "--html".to_string(),
                        ],
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
        LighthouseTarget::Installer => vec![Project::InstallerFrontend],
        LighthouseTarget::Manager => vec![Project::ManagerFrontend],
        LighthouseTarget::All => vec![Project::InstallerFrontend, Project::ManagerFrontend],
    };

    for project in targets {
        let working_dir = root.join(project.relative_path());

        // Combined task: install deps, build, then run Lighthouse CI
        // Using bash to chain commands so they run sequentially
        tasks.push(TaskConfig {
            project,
            name: format!("Lighthouse {}", project.display_name()),
            command: "bash".to_string(),
            args: vec![
                "-c".to_string(),
                "npm ci && npm run build && npx @lhci/cli autorun".to_string(),
            ],
            working_dir,
        });
    }

    tasks
}

async fn run_task(
    task_idx: usize,
    task: Arc<Mutex<TaskState>>,
    tx: mpsc::Sender<TaskMessage>,
) {
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
    cmd.args(&config.args)
        .current_dir(&config.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CI", "true")
        .env("FORCE_COLOR", "0")
        .env("NO_COLOR", "1");

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tx
                .send(TaskMessage::Failed(task_idx, format!("Failed to spawn: {}", e)))
                .await;
            let mut state = task.lock().unwrap();
            state.status = TaskStatus::Failed;
            state.end_time = Some(Instant::now());
            return;
        }
    };

    let stdout = child.stdout;
    let stderr = child.stderr;

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

    stdout_handle.join().ok();
    stderr_handle.join().ok();

    // Wait for process to complete - we need to get ownership of child
    // Since we consumed stdout/stderr, we need to drop them properly
    // Actually we need to handle this differently
    let exit_status = {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .current_dir(&config.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CI", "true")
            .env("FORCE_COLOR", "0")
            .env("NO_COLOR", "1");

        match cmd.output() {
            Ok(output) => {
                // Send any remaining output
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if !line.is_empty() {
                        let mut state = task.lock().unwrap();
                        state.output.push(line.to_string());
                    }
                }
                for line in String::from_utf8_lossy(&output.stderr).lines() {
                    if !line.is_empty() {
                        let mut state = task.lock().unwrap();
                        state.output.push(line.to_string());
                    }
                }
                Some(output.status.code().unwrap_or(-1))
            }
            Err(_) => None,
        }
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
                .send(TaskMessage::Failed(task_idx, "Process terminated".to_string()))
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
    let header_text = format!(
        " Zone Dev Tools - {} ",
        Local::now().format("%H:%M:%S")
    );
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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

    // Task list
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, task)| {
            let state = task.lock().unwrap();
            let (status_icon, status_color) = match state.status {
                TaskStatus::Pending => ("○", Color::DarkGray),
                TaskStatus::Running => ("◐", Color::Yellow),
                TaskStatus::Success => ("✓", Color::Green),
                TaskStatus::Failed => ("✗", Color::Red),
                TaskStatus::Skipped => ("⊘", Color::DarkGray),
            };

            let duration = state.duration_str();

            let style = if idx == app.selected_task {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let content = Line::from(vec![
                Span::styled(format!(" {} ", status_icon), Style::default().fg(status_color)),
                Span::styled(
                    format!("{:<20}", state.config.name),
                    Style::default().fg(if idx == app.selected_task {
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
        if app.selected_task < app.tasks.len() {
            let state = app.tasks[app.selected_task].lock().unwrap();
            let cmd_line = format!(
                "$ {} {}",
                state.config.command,
                state.config.args.join(" ")
            );
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
                let color =
                    if line.contains("error") || line.contains("Error") || line.contains("FAILED") {
                        Color::Red
                    } else if line.contains("warning") || line.contains("Warning") {
                        Color::Yellow
                    } else if line.contains("passed") || line.contains("success") || line.contains("ok")
                    {
                        Color::Green
                    } else {
                        Color::Gray
                    };
                lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(color))));
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
                            KeyCode::Up | KeyCode::Char('k') => {
                                match app.focused_pane {
                                    FocusedPane::Tasks => {
                                        if app.selected_task > 0 {
                                            app.selected_task -= 1;
                                            app.reset_output_scroll();
                                        }
                                    }
                                    FocusedPane::Output => {
                                        app.output_scroll = app.output_scroll.saturating_sub(1);
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                match app.focused_pane {
                                    FocusedPane::Tasks => {
                                        if app.selected_task < app.tasks.len().saturating_sub(1) {
                                            app.selected_task += 1;
                                            app.reset_output_scroll();
                                        }
                                    }
                                    FocusedPane::Output => {
                                        app.output_scroll = app.output_scroll.saturating_add(1);
                                    }
                                }
                            }
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
                                // Calculate which task was clicked (accounting for border)
                                let task_y = y.saturating_sub(app.tasks_area.y + 1);
                                let task_idx = task_y as usize;
                                if task_idx < app.tasks.len() && app.selected_task != task_idx {
                                    app.selected_task = task_idx;
                                    app.reset_output_scroll();
                                }
                            } else if in_output {
                                app.focused_pane = FocusedPane::Output;
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if in_output {
                                app.output_scroll = app.output_scroll.saturating_sub(3);
                            } else if in_tasks && app.selected_task > 0 {
                                app.selected_task -= 1;
                                app.reset_output_scroll();
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if in_output {
                                app.output_scroll = app.output_scroll.saturating_add(3);
                            } else if in_tasks && app.selected_task < app.tasks.len().saturating_sub(1) {
                                app.selected_task += 1;
                                app.reset_output_scroll();
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
    println!(
        "{} Running {} tasks...\n",
        "→".cyan().bold(),
        tasks.len()
    );

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
            all_tasks.extend(create_test_tasks(&root, &projects));
            all_tasks
        }
        Commands::Lighthouse { target } => create_lighthouse_tasks(&root, *target),
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

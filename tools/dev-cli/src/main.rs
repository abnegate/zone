use anyhow::Result;
use chrono::Local;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use owo_colors::OwoColorize;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Project {
    /// Installer frontend (React/TypeScript)
    InstallerFrontend,
    /// Installer backend (Gleam)
    InstallerBackend,
    /// Manager frontend (React/TypeScript)
    ManagerFrontend,
    /// Manager backend (Gleam)
    ManagerBackend,
    /// Runner (Rust)
    Runner,
}

impl Project {
    fn all() -> Vec<Project> {
        vec![
            Project::InstallerFrontend,
            Project::InstallerBackend,
            Project::ManagerFrontend,
            Project::ManagerBackend,
            Project::Runner,
        ]
    }

    fn display_name(&self) -> &'static str {
        match self {
            Project::InstallerFrontend => "Installer Frontend",
            Project::InstallerBackend => "Installer Backend",
            Project::ManagerFrontend => "Manager Frontend",
            Project::ManagerBackend => "Manager Backend",
            Project::Runner => "Runner",
        }
    }

    fn relative_path(&self) -> &'static str {
        match self {
            Project::InstallerFrontend => "installer/frontend",
            Project::InstallerBackend => "installer",
            Project::ManagerFrontend => "manager/frontend",
            Project::ManagerBackend => "manager",
            Project::Runner => "runner",
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

struct App {
    tasks: Vec<Arc<Mutex<TaskState>>>,
    selected_task: usize,
    should_quit: bool,
    all_done: bool,
    start_time: Instant,
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
        }
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
                // Check if biome is available (Manager frontend has it)
                let biome_config = working_dir.join("biome.json");
                if biome_config.exists() {
                    let (cmd, args) = if check {
                        ("npx", vec!["biome", "format", "--check", "src"])
                    } else {
                        ("npx", vec!["biome", "format", "--write", "src"])
                    };
                    tasks.push(TaskConfig {
                        project: *project,
                        name: format!("Format {}", project.display_name()),
                        command: cmd.to_string(),
                        args: args.into_iter().map(String::from).collect(),
                        working_dir,
                    });
                } else {
                    // Installer frontend doesn't have biome, use prettier if available or skip
                    let package_json = working_dir.join("package.json");
                    if package_json.exists() {
                        // Check for prettier in package.json
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
            Project::InstallerBackend | Project::ManagerBackend => {
                let (cmd, args) = if check {
                    ("gleam", vec!["format", "--check", "src", "test"])
                } else {
                    ("gleam", vec!["format", "src", "test"])
                };
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Format {}", project.display_name()),
                    command: cmd.to_string(),
                    args: args.into_iter().map(String::from).collect(),
                    working_dir,
                });
            }
            Project::Runner => {
                let (cmd, args) = if check {
                    ("cargo", vec!["fmt", "--all", "--", "--check"])
                } else {
                    ("cargo", vec!["fmt", "--all"])
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
            Project::InstallerBackend | Project::ManagerBackend => {
                // Gleam check for type errors
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Check {}", project.display_name()),
                    command: "gleam".to_string(),
                    args: vec!["check".to_string()],
                    working_dir,
                });
            }
            Project::Runner => {
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
                    name: format!("Lint {}", project.display_name()),
                    command: "cargo".to_string(),
                    args: args.into_iter().map(String::from).collect(),
                    working_dir,
                });
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
            Project::InstallerBackend | Project::ManagerBackend => {
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Test {}", project.display_name()),
                    command: "gleam".to_string(),
                    args: vec!["test".to_string()],
                    working_dir,
                });
            }
            Project::Runner => {
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Test {}", project.display_name()),
                    command: "cargo".to_string(),
                    args: vec!["test".to_string(), "--all-features".to_string()],
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
            Project::InstallerBackend | Project::ManagerBackend => {
                // Gleam coverage via the custom script if available
                let coverage_script = working_dir.join("scripts/test-coverage.sh");
                if coverage_script.exists() {
                    tasks.push(TaskConfig {
                        project: *project,
                        name: format!("Coverage {}", project.display_name()),
                        command: "bash".to_string(),
                        args: vec!["scripts/test-coverage.sh".to_string()],
                        working_dir,
                    });
                } else {
                    // Just run tests if no coverage script
                    tasks.push(TaskConfig {
                        project: *project,
                        name: format!("Test {}", project.display_name()),
                        command: "gleam".to_string(),
                        args: vec!["test".to_string()],
                        working_dir,
                    });
                }
            }
            Project::Runner => {
                tasks.push(TaskConfig {
                    project: *project,
                    name: format!("Coverage {}", project.display_name()),
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

fn draw_ui(frame: &mut Frame, app: &App) {
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

    // Split the main area into task list and output
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[2]);

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

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Tasks"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, main_chunks[0]);

    // Output panel
    let selected_output: Vec<Line> = if app.selected_task < app.tasks.len() {
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

        let output_start = if output_clone.len() > 50 {
            output_clone.len().saturating_sub(50)
        } else {
            0
        };

        for line in output_clone.iter().skip(output_start) {
            let color = if line.contains("error") || line.contains("Error") || line.contains("FAILED") {
                Color::Red
            } else if line.contains("warning") || line.contains("Warning") {
                Color::Yellow
            } else if line.contains("passed") || line.contains("success") || line.contains("ok") {
                Color::Green
            } else {
                Color::Gray
            };
            lines.push(Line::from(Span::styled(line.clone(), Style::default().fg(color))));
        }

        lines
    } else {
        vec![Line::from("No task selected")]
    };

    let output = Paragraph::new(selected_output)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Output (last 50 lines)"),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    frame.render_widget(output, main_chunks[1]);

    // Footer
    let footer_text = if app.all_done {
        if app.failed_count() > 0 {
            " Press 'q' to quit | ↑↓ to navigate tasks | Some tasks failed! "
        } else {
            " Press 'q' to quit | ↑↓ to navigate tasks | All tasks completed! "
        }
    } else {
        " Press 'q' to quit | ↑↓ to navigate tasks | Running... "
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, chunks[3]);
}

async fn run_tui(tasks: Vec<TaskConfig>) -> Result<bool> {
    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

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
        terminal.draw(|f| draw_ui(f, &app))?;

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

        // Handle input
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.selected_task > 0 {
                                app.selected_task -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.selected_task < app.tasks.len().saturating_sub(1) {
                                app.selected_task += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Cleanup
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

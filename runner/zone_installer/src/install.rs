//! Docker Compose installer HTTP handler.

use axum::{
    body::Body,
    extract::{Json, State},
    http::{StatusCode, header},
    response::IntoResponse,
    response::Response,
};
use bytes::Bytes;
use futures::StreamExt;
use rand::RngExt;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::{fs, process::Command, sync::mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::serve::AppState;

const INSTALLER_SHUTDOWN_DELAY_SECS: u64 = 8;
const COMPOSE_UP_TIMEOUT_SECS: u64 = 1200;
const COMPOSE_HEARTBEAT_SECS: u64 = 15;
const COMPOSE_OUTPUT_TAIL: usize = 12;

type InstallerConfig = HashMap<String, String>;

/// Handle the installation request
pub(crate) async fn handle_install(
    State(_state): State<AppState>,
    Json(config): Json<InstallerConfig>,
) -> impl IntoResponse {
    let (tx, rx) = mpsc::channel::<InstallUpdate>(32);

    tokio::spawn(async move {
        run_install(config, tx).await;
    });

    let stream = ReceiverStream::new(rx).map(|update| {
        let line = serde_json::to_string(&update).unwrap_or_else(|_| {
            serde_json::json!({
                "status": "Failed to serialize update",
                "error": true
            })
            .to_string()
        });
        Ok::<Bytes, Infallible>(Bytes::from(format!("{line}\n")))
    });

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn run_install(config: InstallerConfig, tx: mpsc::Sender<InstallUpdate>) {
    let send = |update: InstallUpdate| async {
        let _ = tx.send(update).await;
    };

    send(InstallUpdate::step_done(
        "config",
        10,
        "Configuration received",
    ))
    .await;
    send(InstallUpdate::step_start(
        "env",
        30,
        "Creating .env file...",
    ))
    .await;

    match create_env_from_config(&config).await {
        Ok(_) => {
            send(InstallUpdate::step_done("env", 40, ".env file created")).await;
        }
        Err(e) => {
            send(InstallUpdate::step_error(
                "env",
                30,
                &format!("Failed to create .env: {}", e),
            ))
            .await;
            return;
        }
    }

    send(InstallUpdate::step_start(
        "auth-dir",
        50,
        "Creating auth directory...",
    ))
    .await;
    if let Err(e) = fs::create_dir_all("./auth").await {
        send(InstallUpdate::step_error(
            "auth-dir",
            50,
            &format!("Failed to create auth directory: {}", e),
        ))
        .await;
        return;
    }
    send(InstallUpdate::step_done(
        "auth-dir",
        60,
        "Auth directory created",
    ))
    .await;

    send(InstallUpdate::step_start(
        "auth-creds",
        70,
        "Generating admin credentials...",
    ))
    .await;
    match create_auth_file().await {
        Ok(_) => {
            send(InstallUpdate::step_done(
                "auth-creds",
                90,
                "Admin credentials created",
            ))
            .await;
        }
        Err(e) => {
            send(InstallUpdate::step_error(
                "auth-creds",
                70,
                &format!("Failed to create auth file: {}", e),
            ))
            .await;
            return;
        }
    }

    send(InstallUpdate::step_start(
        "compose",
        95,
        "Starting Docker Compose stack...",
    ))
    .await;

    match start_compose_stack(&tx).await {
        Ok(timed_out) => {
            let message = if timed_out {
                "Docker Compose stack started (some services still starting)"
            } else {
                "Docker Compose stack started"
            };
            send(InstallUpdate::step_done("compose", 98, message)).await;
        }
        Err(e) => {
            send(InstallUpdate::step_error(
                "compose",
                95,
                &format!("Failed to start Docker Compose stack: {}", e),
            ))
            .await;
            return;
        }
    }

    send(InstallUpdate::complete(100, "Installation complete!")).await;
    schedule_shutdown();
}

/// Create the .env file from .env.example and apply installer values.
async fn create_env_from_config(config: &InstallerConfig) -> Result<(), std::io::Error> {
    let env_file = Path::new(".env");
    let env_example = Path::new(".env.example");

    let env_example_content = if env_example.exists() {
        Some(fs::read_to_string(env_example).await?)
    } else {
        None
    };

    let mut content = if env_file.exists() {
        fs::read_to_string(env_file).await?
    } else if let Some(example) = env_example_content.as_ref() {
        example.clone()
    } else {
        "# Generated by Zone Web Installer\n".to_string()
    };

    let env_keys = if let Some(example) = env_example_content.as_ref() {
        extract_env_keys(example)
    } else {
        extract_env_keys(&content)
    };

    for (key, value) in config {
        if env_keys.contains(key) {
            content = replace_env_value(&content, key, value);
        }
    }

    if let Some(domain) = config.get("DOMAIN_HOST_WEBUI")
        && !domain.trim().is_empty()
    {
        content = replace_env_value(
            &content,
            "WEBUI_CORS_ALLOW_ORIGIN",
            &format!("http://{}", domain.trim()),
        );
    }

    if let Some(value) = config.get("AI_MODEL_FAST") {
        content = replace_env_value(&content, "OLLAMA_MODEL_FAST", value);
    }
    if let Some(value) = config.get("AI_MODEL_REASONING") {
        content = replace_env_value(&content, "OLLAMA_MODEL_REASON", value);
    }
    if let Some(value) = config.get("AI_MODEL_EMBEDDING") {
        content = replace_env_value(&content, "OLLAMA_MODEL_EMBED", value);
    }

    content = ensure_env_value(&content, "SECURITY_LITELLM_UI_USERNAME", "");
    content = ensure_env_value(&content, "SECURITY_LITELLM_UI_PASSWORD", "");
    content = ensure_env_secret(&content, "JWT_SECRET");
    content = ensure_env_secret(&content, "ENCRYPTION_KEY");
    content = ensure_urlencoded_secret(
        &content,
        "POSTGRES_PASSWORD",
        "POSTGRES_PASSWORD_URLENCODED",
    );

    fs::write(".env", content).await
}

/// Create the htpasswd file with a generated admin password
async fn create_auth_file() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let password = generate_secure_password();

    // Try to use htpasswd command for bcrypt hash
    let htpasswd_content = create_htpasswd_entry("admin", &password).await?;

    // Write htpasswd file
    fs::write("./auth/users.htpasswd", &htpasswd_content).await?;

    // Write password reference file
    let password_info = format!(
        "# GENERATED ADMIN CREDENTIALS\n\
         # Username: admin\n\
         # Password: {}\n\n\
         # IMPORTANT: Save this password securely and delete this file!\n\
         # You can change the password later with: htpasswd -B auth/users.htpasswd admin\n",
        password
    );
    fs::write("./auth/ADMIN_PASSWORD.txt", password_info).await?;

    Ok(())
}

/// Generate a cryptographically secure random password
fn generate_secure_password() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();

    (0..24)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Create an htpasswd entry using the htpasswd command (bcrypt)
async fn create_htpasswd_entry(
    username: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("htpasswd")
        .args(["-nbB", username, password])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => {
            let result = String::from_utf8_lossy(&output.stdout).to_string();
            Ok(result)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("htpasswd failed: {}, using fallback", stderr);
            // Fallback if htpasswd fails
            Ok(format!(
                "{}:{}\n# WARNING: Password stored in plaintext - htpasswd not available\n",
                username, password
            ))
        }
        Err(e) => {
            tracing::warn!("htpasswd command not found: {}, using fallback", e);
            // Fallback if htpasswd is not installed
            Ok(format!(
                "{}:{}\n# WARNING: Password stored in plaintext - htpasswd not available\n",
                username, password
            ))
        }
    }
}

fn extract_env_keys(content: &str) -> HashSet<String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, _) = trimmed.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                None
            } else {
                Some(key.to_string())
            }
        })
        .collect()
}

fn get_env_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == key
        {
            let value = v.trim();
            let value = value.trim_matches('"').trim_matches('\'');
            return Some(value.to_string());
        }
    }
    None
}

fn replace_env_value(content: &str, key: &str, value: &str) -> String {
    let mut result = String::new();
    let mut found = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('#')
            && !trimmed.is_empty()
            && let Some((k, _)) = trimmed.split_once('=')
            && k.trim() == key
        {
            result.push_str(&format!("{}={}\n", key, value));
            found = true;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if !found {
        result.push_str(&format!("{}={}\n", key, value));
    }

    result
}

fn ensure_env_value(content: &str, key: &str, value: &str) -> String {
    if get_env_value(content, key).is_some() {
        content.to_string()
    } else {
        replace_env_value(content, key, value)
    }
}

fn ensure_env_secret(content: &str, key: &str) -> String {
    match get_env_value(content, key) {
        Some(existing) if !existing.is_empty() => content.to_string(),
        _ => replace_env_value(content, key, &generate_secret()),
    }
}

fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

fn url_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        let is_unreserved =
            matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~');
        if is_unreserved {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    encoded
}

fn ensure_urlencoded_secret(content: &str, raw_key: &str, encoded_key: &str) -> String {
    let raw_value = get_env_value(content, raw_key).unwrap_or_default();
    if raw_value.is_empty() {
        return ensure_env_value(content, encoded_key, "");
    }

    let encoded = url_encode(&raw_value);
    replace_env_value(content, encoded_key, &encoded)
}

fn schedule_shutdown() {
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(INSTALLER_SHUTDOWN_DELAY_SECS)).await;
        tracing::info!("Installation complete. Shutting down installer.");
        std::process::exit(0);
    });
}

async fn start_compose_stack(tx: &mpsc::Sender<InstallUpdate>) -> Result<bool, String> {
    let project_root = resolve_compose_project_root()?;
    let docker_context = ComposeContext::docker(project_root.clone());
    let up_args = ["up", "-d", "--build"];
    let docker_result = run_compose_command(&docker_context, &up_args, tx).await;

    if docker_result.is_ok() {
        return monitor_compose_services(tx, &docker_context).await;
    }

    let docker_compose_context = ComposeContext::docker_compose(project_root.clone());
    let _ = tx
        .send(InstallUpdate::with_state(
            95,
            "docker compose failed, trying docker-compose...",
            Some("compose"),
            Some("in-progress"),
            false,
            None,
        ))
        .await;
    let docker_compose_result = run_compose_command(&docker_compose_context, &up_args, tx).await;

    if docker_compose_result.is_ok() {
        return monitor_compose_services(tx, &docker_compose_context).await;
    }

    Err(format!(
        "docker compose failed ({}) and docker-compose failed ({})",
        docker_result.unwrap_err(),
        docker_compose_result.unwrap_err()
    ))
}

fn resolve_compose_project_root() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("ZONE_HOST_PROJECT_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() && trimmed != "." {
            let path = PathBuf::from(trimmed);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    std::env::current_dir().map_err(|err| format!("Failed to resolve working directory: {}", err))
}

struct ComposeContext {
    program: &'static str,
    args_prefix: Vec<String>,
    working_dir: PathBuf,
}

impl ComposeContext {
    fn docker(project_root: PathBuf) -> Self {
        let project_root_str = project_root.to_string_lossy();
        Self {
            program: "docker",
            args_prefix: vec![
                "compose".to_string(),
                "--project-directory".to_string(),
                project_root_str.to_string(),
            ],
            working_dir: project_root,
        }
    }

    fn docker_compose(project_root: PathBuf) -> Self {
        let compose_file = project_root.join("docker-compose.yml");
        let compose_file_str = compose_file.to_string_lossy();
        Self {
            program: "docker-compose",
            args_prefix: vec!["-f".to_string(), compose_file_str.to_string()],
            working_dir: project_root,
        }
    }
}

async fn run_compose_command(
    context: &ComposeContext,
    args: &[&str],
    tx: &mpsc::Sender<InstallUpdate>,
) -> Result<(), String> {
    let mut full_args = context.args_prefix.clone();
    full_args.extend(args.iter().map(|arg| (*arg).to_string()));

    let mut child = Command::new(context.program)
        .args(&full_args)
        .current_dir(&context.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture stderr".to_string())?;
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut exit_status: Option<std::process::ExitStatus> = None;

    let start = Instant::now();
    let mut last_output = Instant::now();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(COMPOSE_HEARTBEAT_SECS));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_update: Option<String> = None;

    let mut stdout_tail: VecDeque<String> = VecDeque::with_capacity(COMPOSE_OUTPUT_TAIL);
    let mut stderr_tail: VecDeque<String> = VecDeque::with_capacity(COMPOSE_OUTPUT_TAIL);

    loop {
        tokio::select! {
            line = stdout_lines.next_line(), if !stdout_done => {
                match line {
                    Ok(Some(line)) => {
                        last_output = Instant::now();
                        push_compose_tail(&mut stdout_tail, &line);
                        if let Some(message) = sanitize_compose_output(&line) {
                            send_compose_update(tx, &message, &mut last_update).await;
                        }
                    }
                    Ok(None) => {
                        stdout_done = true;
                    }
                    Err(err) => {
                        stdout_done = true;
                        push_compose_tail(&mut stderr_tail, &format!("stdout read failed: {}", err));
                    }
                }
            }
            line = stderr_lines.next_line(), if !stderr_done => {
                match line {
                    Ok(Some(line)) => {
                        last_output = Instant::now();
                        push_compose_tail(&mut stderr_tail, &line);
                        if let Some(message) = sanitize_compose_output(&line) {
                            send_compose_update(tx, &message, &mut last_update).await;
                        }
                    }
                    Ok(None) => {
                        stderr_done = true;
                    }
                    Err(err) => {
                        stderr_done = true;
                        push_compose_tail(&mut stderr_tail, &format!("stderr read failed: {}", err));
                    }
                }
            }
            status = child.wait(), if exit_status.is_none() => {
                exit_status = Some(status.map_err(|err| err.to_string())?);
            }
            _ = heartbeat.tick() => {
                if start.elapsed() >= Duration::from_secs(COMPOSE_UP_TIMEOUT_SECS) {
                    let _ = child.kill().await;
                    return Err(format!(
                        "docker compose timed out after {} seconds",
                        COMPOSE_UP_TIMEOUT_SECS
                    ));
                }

                if last_output.elapsed() >= Duration::from_secs(COMPOSE_HEARTBEAT_SECS) {
                    let message = format!(
                        "Docker Compose still running ({}s elapsed)...",
                        start.elapsed().as_secs()
                    );
                    send_compose_update(tx, &message, &mut last_update).await;
                }
            }
        }

        if exit_status.is_some() && stdout_done && stderr_done {
            break;
        }
    }

    let status = exit_status.unwrap();
    if status.success() {
        return Ok(());
    }

    let details = if !stderr_tail.is_empty() {
        stderr_tail.into_iter().collect::<Vec<_>>().join(" | ")
    } else if !stdout_tail.is_empty() {
        stdout_tail.into_iter().collect::<Vec<_>>().join(" | ")
    } else {
        status
            .code()
            .map(|code| format!("exit code {}", code))
            .unwrap_or_else(|| "unknown error".to_string())
    };

    Err(details)
}

async fn monitor_compose_services(
    tx: &mpsc::Sender<InstallUpdate>,
    context: &ComposeContext,
) -> Result<bool, String> {
    let services = match list_compose_services(context).await {
        Ok(services) => services,
        Err(_) => return Ok(true),
    };
    if services.is_empty() {
        return Ok(false);
    }

    let mut last_messages: HashMap<String, String> = HashMap::new();
    let mut last_states: HashMap<String, String> = HashMap::new();

    let poll_interval = std::time::Duration::from_secs(2);
    let timeout = std::time::Duration::from_secs(180);
    let start = std::time::Instant::now();

    loop {
        let mut all_ready = true;
        let mut failed_services: Vec<String> = Vec::new();

        for service in &services {
            let status = inspect_compose_service(context, service).await?;
            let (state, message, ready, failed) = map_service_state(service, &status);

            let id = format!("compose:{}", service);
            let last_message = last_messages.get(&id);
            let last_state = last_states.get(&id);
            if last_message != Some(&message) || last_state != Some(&state) {
                last_messages.insert(id.clone(), message.clone());
                last_states.insert(id.clone(), state.clone());
                let update = InstallUpdate::with_state(
                    95,
                    &message,
                    Some(&id),
                    Some(&state),
                    state == "error",
                    None,
                );
                let _ = tx.send(update).await;
            }

            if failed {
                failed_services.push(service.clone());
            }
            if !ready {
                all_ready = false;
            }
        }

        if !failed_services.is_empty() {
            return Err(format!(
                "services failed to start: {}",
                failed_services.join(", ")
            ));
        }

        if all_ready {
            return Ok(false);
        }

        if start.elapsed() >= timeout {
            return Ok(true);
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn list_compose_services(context: &ComposeContext) -> Result<Vec<String>, String> {
    let output = run_compose_command_output(context, &["ps", "--services"]).await;
    let fallback_output = if let Ok(output) = output {
        if output.trim().is_empty() {
            run_compose_command_output(context, &["config", "--services"]).await?
        } else {
            output
        }
    } else {
        run_compose_command_output(context, &["config", "--services"]).await?
    };

    Ok(fallback_output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect())
}

async fn inspect_compose_service(
    context: &ComposeContext,
    service: &str,
) -> Result<ServiceInspect, String> {
    let id_output = match run_compose_command_output(context, &["ps", "-q", service]).await {
        Ok(output) => output,
        Err(_) => return Ok(ServiceInspect::default()),
    };
    let id = id_output.lines().next().unwrap_or("").trim().to_string();
    if id.is_empty() {
        return Ok(ServiceInspect::default());
    }

    let format =
        "{{.State.Status}}|{{if .State.Health}}{{.State.Health.Status}}{{end}}|{{.State.ExitCode}}";
    let inspect_output = Command::new("docker")
        .args(["inspect", "--format", format, &id])
        .current_dir(&context.working_dir)
        .output()
        .await
        .map_err(|err| err.to_string())?;

    if !inspect_output.status.success() {
        return Ok(ServiceInspect::default());
    }

    let output = String::from_utf8_lossy(&inspect_output.stdout);
    let parts: Vec<&str> = output.trim().split('|').collect();
    Ok(ServiceInspect {
        status: parts.first().unwrap_or(&"").to_string(),
        health: parts.get(1).unwrap_or(&"").to_string(),
        exit_code: parts
            .get(2)
            .and_then(|code| code.parse::<i32>().ok())
            .unwrap_or(0),
        exists: true,
    })
}

async fn run_compose_command_output(
    context: &ComposeContext,
    args: &[&str],
) -> Result<String, String> {
    let mut full_args = context.args_prefix.clone();
    full_args.extend(args.iter().map(|arg| (*arg).to_string()));

    let output = Command::new(context.program)
        .args(&full_args)
        .current_dir(&context.working_dir)
        .output()
        .await
        .map_err(|err| err.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "command failed".to_string()
        } else {
            stderr
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn push_compose_tail(tail: &mut VecDeque<String>, line: &str) {
    if tail.len() >= COMPOSE_OUTPUT_TAIL {
        tail.pop_front();
    }
    tail.push_back(line.trim().to_string());
}

fn sanitize_compose_output(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let max_len = 220;
    if trimmed.len() <= max_len {
        return Some(trimmed.to_string());
    }

    let mut shortened = trimmed.chars().take(max_len).collect::<String>();
    shortened.push_str("...");
    Some(shortened)
}

async fn send_compose_update(
    tx: &mpsc::Sender<InstallUpdate>,
    message: &str,
    last_message: &mut Option<String>,
) {
    if last_message.as_deref() == Some(message) {
        return;
    }
    *last_message = Some(message.to_string());
    let _ = tx
        .send(InstallUpdate::with_state(
            95,
            message,
            Some("compose"),
            Some("in-progress"),
            false,
            None,
        ))
        .await;
}

#[derive(Default)]
struct ServiceInspect {
    status: String,
    health: String,
    exit_code: i32,
    exists: bool,
}

fn map_service_state(service: &str, inspect: &ServiceInspect) -> (String, String, bool, bool) {
    if !inspect.exists || inspect.status.is_empty() {
        return (
            "in-progress".to_string(),
            format!("{}: pending", service),
            false,
            false,
        );
    }

    let status = inspect.status.as_str();
    let health = inspect.health.as_str();

    match status {
        "running" => {
            if health == "unhealthy" {
                (
                    "error".to_string(),
                    format!("{}: unhealthy", service),
                    false,
                    true,
                )
            } else if health == "healthy" || health.is_empty() {
                (
                    "success".to_string(),
                    format!("{}: running", service),
                    true,
                    false,
                )
            } else {
                (
                    "in-progress".to_string(),
                    format!("{}: starting", service),
                    false,
                    false,
                )
            }
        }
        "created" | "restarting" | "removing" | "paused" => (
            "in-progress".to_string(),
            format!("{}: {}", service, status),
            false,
            false,
        ),
        "exited" => {
            if inspect.exit_code == 0 {
                (
                    "success".to_string(),
                    format!("{}: completed", service),
                    true,
                    false,
                )
            } else {
                (
                    "error".to_string(),
                    format!("{}: exited ({})", service, inspect.exit_code),
                    false,
                    true,
                )
            }
        }
        _ => (
            "in-progress".to_string(),
            format!("{}: {}", service, status),
            false,
            false,
        ),
    }
}

/// Installation update message
#[derive(serde::Serialize)]
struct InstallUpdate {
    progress: u8,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<bool>,
}

impl InstallUpdate {
    fn with_state(
        progress: u8,
        status: &str,
        id: Option<&str>,
        state: Option<&str>,
        is_error: bool,
        complete: Option<bool>,
    ) -> Self {
        Self {
            progress,
            status: status.to_string(),
            id: id.map(|value| value.to_string()),
            state: state.map(|value| value.to_string()),
            complete,
            error: if is_error { Some(true) } else { None },
        }
    }

    fn step_start(id: &str, progress: u8, status: &str) -> Self {
        Self::with_state(progress, status, Some(id), Some("in-progress"), false, None)
    }

    fn step_done(id: &str, progress: u8, status: &str) -> Self {
        Self::with_state(progress, status, Some(id), Some("success"), false, None)
    }

    fn step_error(id: &str, progress: u8, status: &str) -> Self {
        Self::with_state(progress, status, Some(id), Some("error"), true, None)
    }

    fn complete(progress: u8, status: &str) -> Self {
        Self::with_state(progress, status, None, Some("success"), false, Some(true))
    }
}

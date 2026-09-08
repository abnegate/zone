//! OS-level confinement for spawned commands.
//!
//! A confined command runs under the host sandbox backend — seatbelt on macOS,
//! bubblewrap on Linux — with an explicit set of readable and writable roots and
//! no network access at all.
//!
//! Confinement is never assumed to work. [`Confinement::probe`] executes real
//! commands inside the sandbox and asserts that a denied file stays unreadable
//! and that a connection to a live local listener never arrives. A host that
//! cannot prove those properties refuses to run confined jobs rather than
//! running them unconfined.

use crate::protocol::ConfinementRequest;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::OnceCell;
use tokio::time::timeout;

const SEATBELT: &str = "/usr/bin/sandbox-exec";
const BUBBLEWRAP: &str = "/usr/bin/bwrap";

const SEATBELT_READ_TREES: [&str; 4] = ["/System", "/dev", "/usr/lib", "/usr/share"];

/// Name resolution and account enumeration stay denied even though `system.sb`
/// grants a broad read of the system volume.
const SEATBELT_DENIED_FILES: [&str; 2] = ["/private/etc/hosts", "/private/etc/passwd"];

const BUBBLEWRAP_READ_TREES: [&str; 4] = ["/usr", "/bin", "/lib", "/lib64"];
const BUBBLEWRAP_LOADER_FILES: [&str; 3] =
    ["/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/localtime"];

const HOME: &str = "HOME";
const TMPDIR: &str = "TMPDIR";
const TMP: &str = "TMP";
const TEMP: &str = "TEMP";
const PATH: &str = "PATH";
const LANG: &str = "LANG";
const LC_ALL: &str = "LC_ALL";

const SYSTEM_PATH: &str = "/usr/bin:/bin";
const LOCALE: &str = "C.UTF-8";

const PROBE_COMMAND: &str = "/bin/cat";
const PROBE_ALLOWED: &str = "allowed";
const PROBE_DENIED: &str = "denied";
const PROBE_ALLOWED_CONTENT: &str = "allowed\n";
const PROBE_DENIED_CONTENT: &str = "secret\n";
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_SETTLE: Duration = Duration::from_millis(25);

/// Commands able to open a TCP connection, in preference order. The probe needs
/// exactly one of them to exist; a host with none cannot prove its sandbox.
const NETWORK_PROBE_COMMANDS: [&str; 5] = [
    "/usr/bin/nc",
    "/bin/nc",
    "/usr/bin/ncat",
    "/usr/bin/curl",
    "/bin/curl",
];

/// Errors raised while preparing or proving confinement.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ConfinementError {
    #[error("Confinement is not supported on this platform")]
    UnsupportedPlatform,

    #[error("Confinement backend is not an executable file: {0}")]
    BackendMissing(String),

    #[error("Confined path must be absolute: {0}")]
    RelativePath(String),

    #[error("Confined path is not valid UTF-8: {0}")]
    NonUnicodePath(String),

    #[error("Confined path contains a control character: {0}")]
    ControlCharacterInPath(String),

    #[error("Confined path is unusable: {path}: {reason}")]
    UnusablePath { path: String, reason: String },

    #[error("Confined command not found: {0}")]
    CommandNotFound(String),

    #[error("Confinement could not be proven: {0}")]
    Unproven(String),
}

/// The OS mechanism used to confine a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Seatbelt,
    Bubblewrap,
}

#[cfg(target_os = "macos")]
pub const HOST_BACKEND: Option<Backend> = Some(Backend::Seatbelt);

#[cfg(target_os = "linux")]
pub const HOST_BACKEND: Option<Backend> = Some(Backend::Bubblewrap);

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const HOST_BACKEND: Option<Backend> = None;

impl Backend {
    /// Absolute path of the backend executable.
    pub const fn executable(self) -> &'static str {
        match self {
            Backend::Seatbelt => SEATBELT,
            Backend::Bubblewrap => BUBBLEWRAP,
        }
    }

    /// A file the sandbox must refuse to read, used by the probe.
    const fn denied_system_file(self) -> &'static str {
        match self {
            Backend::Seatbelt => SEATBELT_DENIED_FILES[0],
            Backend::Bubblewrap => "/etc/hosts",
        }
    }
}

/// A backend executable and the argument vector that runs a command inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub arguments: Vec<String>,
    /// Environment for the backend process itself. Bubblewrap clears its own
    /// environment and carries the command's through `--setenv`, so this is
    /// empty there and complete under seatbelt.
    pub environment: BTreeMap<String, String>,
}

/// A command together with the filesystem it is allowed to see.
#[derive(Debug, Clone)]
pub struct Confinement {
    command: String,
    arguments: Vec<String>,
    working_dir: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    environment: BTreeMap<String, String>,
}

struct Resolved {
    command: PathBuf,
    arguments: Vec<String>,
    working_dir: PathBuf,
    read_roots: Vec<PathBuf>,
    write_roots: Vec<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl Confinement {
    pub fn new(
        command: impl Into<String>,
        arguments: Vec<String>,
        working_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            command: command.into(),
            arguments,
            working_dir: working_dir.into(),
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            environment: BTreeMap::new(),
        }
    }

    /// Grant the roots named by a protocol request.
    pub fn with_roots(mut self, request: &ConfinementRequest) -> Self {
        self.read_roots = request.read_roots.clone();
        self.write_roots = request.write_roots.clone();
        self
    }

    /// Set the environment handed to the confined command.
    pub fn with_environment(mut self, environment: HashMap<String, String>) -> Self {
        self.environment = environment.into_iter().collect();
        self
    }

    /// Whether this host has a confinement backend installed. Availability is
    /// not proof: [`Confinement::probe`] still has to pass before a confined
    /// job may run.
    pub fn is_available() -> bool {
        HOST_BACKEND.is_some_and(|backend| backend_executable(backend).is_ok())
    }

    /// Translate into a backend invocation. `None` means the host has no
    /// backend, which is an error rather than an unconfined spawn.
    pub fn invocation(&self, backend: Option<Backend>) -> Result<Invocation, ConfinementError> {
        let backend = backend.ok_or(ConfinementError::UnsupportedPlatform)?;
        let resolved = self.resolve()?;
        match backend {
            Backend::Seatbelt => Ok(Invocation {
                program: PathBuf::from(backend.executable()),
                arguments: seatbelt_arguments(&resolved)?,
                environment: resolved.environment,
            }),
            Backend::Bubblewrap => Ok(Invocation {
                program: PathBuf::from(backend.executable()),
                arguments: bubblewrap_arguments(&resolved)?,
                environment: BTreeMap::new(),
            }),
        }
    }

    /// Translate into an invocation for the backend of the running host.
    pub fn host_invocation(&self) -> Result<Invocation, ConfinementError> {
        self.invocation(HOST_BACKEND)
    }

    /// Prove that this host's confinement actually confines, caching the
    /// verdict for the lifetime of the process.
    pub async fn probe() -> Result<(), ConfinementError> {
        static OUTCOME: OnceCell<Result<(), ConfinementError>> = OnceCell::const_new();
        OUTCOME.get_or_init(run_probe).await.clone()
    }

    fn resolve(&self) -> Result<Resolved, ConfinementError> {
        let command = resolve_command(&self.command, &self.environment)?;
        let working_dir = canonical(&self.working_dir)?;
        let read_roots = canonical_roots(&self.read_roots)?;
        let write_roots = canonical_roots(&self.write_roots)?;
        let environment = complete_environment(
            &self.environment,
            &command,
            write_roots.first().unwrap_or(&working_dir),
        );

        Ok(Resolved {
            command,
            arguments: self.arguments.clone(),
            working_dir,
            read_roots,
            write_roots,
            environment,
        })
    }
}

fn seatbelt_arguments(resolved: &Resolved) -> Result<Vec<String>, ConfinementError> {
    let command = text(&resolved.command)?;
    let mut arguments = vec![
        "-p".to_string(),
        seatbelt_profile(resolved)?,
        "--".to_string(),
        command.to_string(),
    ];
    arguments.extend(resolved.arguments.iter().cloned());
    Ok(arguments)
}

fn seatbelt_profile(resolved: &Resolved) -> Result<String, ConfinementError> {
    let command = text(&resolved.command)?;

    let mut trees: BTreeSet<&str> = SEATBELT_READ_TREES.into_iter().collect();
    for root in resolved.read_roots.iter().chain(&resolved.write_roots) {
        trees.insert(text(root)?);
    }

    let mut metadata: BTreeSet<&Path> = BTreeSet::new();
    for path in trees
        .iter()
        .map(Path::new)
        .chain([resolved.command.as_path()])
    {
        metadata.extend(
            path.ancestors()
                .filter(|ancestor| ancestor.parent().is_some()),
        );
    }

    let mut lines = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(import \"system.sb\")".to_string(),
        format!("(allow process-exec (literal {}))", escape(command)),
        "(deny signal)".to_string(),
        "(allow sysctl-read)".to_string(),
    ];
    for tree in &trees {
        lines.push(format!("(allow file-read* (subpath {}))", escape(tree)));
    }
    lines.push(format!("(allow file-read* (literal {}))", escape(command)));
    for path in &metadata {
        lines.push(format!(
            "(allow file-read-metadata (literal {}))",
            escape(text(path)?)
        ));
    }
    for root in &resolved.write_roots {
        lines.push(format!(
            "(allow file-write* (subpath {}))",
            escape(text(root)?)
        ));
    }
    lines.push("(deny network*)".to_string());
    for path in SEATBELT_DENIED_FILES {
        lines.push(format!("(deny file-read* (literal {}))", escape(path)));
    }

    Ok(lines.join("\n"))
}

fn bubblewrap_arguments(resolved: &Resolved) -> Result<Vec<String>, ConfinementError> {
    let mut arguments: Vec<String> = [
        "--die-with-parent",
        "--new-session",
        "--unshare-all",
        "--unshare-net",
        "--clearenv",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect();

    for tree in BUBBLEWRAP_READ_TREES {
        arguments.extend([
            "--ro-bind-try".to_string(),
            tree.to_string(),
            tree.to_string(),
        ]);
    }
    arguments.extend(["--dir".to_string(), "/etc".to_string()]);
    for file in BUBBLEWRAP_LOADER_FILES {
        arguments.extend([
            "--ro-bind-try".to_string(),
            file.to_string(),
            file.to_string(),
        ]);
    }
    for root in &resolved.read_roots {
        let path = text(root)?.to_string();
        arguments.extend(["--ro-bind".to_string(), path.clone(), path]);
    }
    for root in &resolved.write_roots {
        let path = text(root)?.to_string();
        arguments.extend(["--bind".to_string(), path.clone(), path]);
    }
    for (name, value) in &resolved.environment {
        arguments.extend(["--setenv".to_string(), name.clone(), value.clone()]);
    }
    arguments.extend([
        "--chdir".to_string(),
        text(&resolved.working_dir)?.to_string(),
        "--".to_string(),
        text(&resolved.command)?.to_string(),
    ]);
    arguments.extend(resolved.arguments.iter().cloned());

    Ok(arguments)
}

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        if character == '"' || character == '\\' {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('"');
    escaped
}

fn text(path: &Path) -> Result<&str, ConfinementError> {
    let value = path
        .to_str()
        .ok_or_else(|| ConfinementError::NonUnicodePath(path.display().to_string()))?;
    if !path.is_absolute() {
        return Err(ConfinementError::RelativePath(value.to_string()));
    }
    if value.chars().any(char::is_control) {
        return Err(ConfinementError::ControlCharacterInPath(value.to_string()));
    }
    Ok(value)
}

fn canonical(path: &Path) -> Result<PathBuf, ConfinementError> {
    let canonical = fs::canonicalize(path).map_err(|error| ConfinementError::UnusablePath {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    text(&canonical)?;
    Ok(canonical)
}

fn canonical_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, ConfinementError> {
    let mut canonical_roots: Vec<PathBuf> = Vec::with_capacity(roots.len());
    for root in roots {
        let root = canonical(root)?;
        if !canonical_roots.contains(&root) {
            canonical_roots.push(root);
        }
    }
    Ok(canonical_roots)
}

fn backend_executable(backend: Backend) -> Result<PathBuf, ConfinementError> {
    let path = Path::new(backend.executable());
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
            Ok(path.to_path_buf())
        }
        _ => Err(ConfinementError::BackendMissing(path.display().to_string())),
    }
}

fn executable_file(path: &Path) -> Option<PathBuf> {
    let canonical = fs::canonicalize(path).ok()?;
    let metadata = fs::metadata(&canonical).ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(canonical)
}

fn resolve_command(
    command: &str,
    environment: &BTreeMap<String, String>,
) -> Result<PathBuf, ConfinementError> {
    if command.contains('/') {
        return executable_file(Path::new(command))
            .ok_or_else(|| ConfinementError::CommandNotFound(command.to_string()))
            .and_then(|path| {
                text(&path)?;
                Ok(path)
            });
    }

    let search = environment
        .get(PATH)
        .cloned()
        .or_else(|| std::env::var(PATH).ok())
        .unwrap_or_else(|| SYSTEM_PATH.to_string());

    search
        .split(':')
        .filter(|directory| !directory.is_empty())
        .find_map(|directory| executable_file(&Path::new(directory).join(command)))
        .ok_or_else(|| ConfinementError::CommandNotFound(command.to_string()))
}

fn complete_environment(
    requested: &BTreeMap<String, String>,
    command: &Path,
    writable: &Path,
) -> BTreeMap<String, String> {
    let mut environment = requested.clone();
    let writable = writable.display().to_string();
    for name in [HOME, TMPDIR, TMP, TEMP] {
        environment
            .entry(name.to_string())
            .or_insert_with(|| writable.clone());
    }
    environment
        .entry(PATH.to_string())
        .or_insert_with(|| command_path(command));
    for name in [LANG, LC_ALL] {
        environment
            .entry(name.to_string())
            .or_insert_with(|| LOCALE.to_string());
    }
    environment
}

fn command_path(command: &Path) -> String {
    let mut entries: Vec<&str> = Vec::with_capacity(3);
    if let Some(directory) = command.parent().and_then(Path::to_str) {
        entries.push(directory);
    }
    for entry in SYSTEM_PATH.split(':') {
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries.join(":")
}

struct ProbeWorkspace {
    base: PathBuf,
    root: PathBuf,
    denied: PathBuf,
}

impl ProbeWorkspace {
    fn create() -> Result<Self, ConfinementError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        let base = std::env::temp_dir().join(format!(
            "tool-runner-confinement-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&base).map_err(probe_failure)?;
        let base = fs::canonicalize(&base).map_err(probe_failure)?;
        let root = base.join("root");
        fs::create_dir(&root).map_err(probe_failure)?;
        fs::write(root.join(PROBE_ALLOWED), PROBE_ALLOWED_CONTENT).map_err(probe_failure)?;
        let denied = base.join(PROBE_DENIED);
        fs::write(&denied, PROBE_DENIED_CONTENT).map_err(probe_failure)?;
        Ok(Self { base, root, denied })
    }
}

impl Drop for ProbeWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn probe_failure(error: std::io::Error) -> ConfinementError {
    ConfinementError::Unproven(error.to_string())
}

async fn run_probe() -> Result<(), ConfinementError> {
    let backend = HOST_BACKEND.ok_or(ConfinementError::UnsupportedPlatform)?;
    backend_executable(backend)?;
    if executable_file(Path::new(PROBE_COMMAND)).is_none() {
        return Err(ConfinementError::Unproven(format!(
            "probe command {PROBE_COMMAND} is unavailable"
        )));
    }

    let workspace = ProbeWorkspace::create()?;

    let allowed = run_confined(
        backend,
        &workspace.root,
        PROBE_COMMAND,
        vec![text(&workspace.root.join(PROBE_ALLOWED))?.to_string()],
    )
    .await?;
    if !allowed.status.success() || allowed.stdout != PROBE_ALLOWED_CONTENT.as_bytes() {
        return Err(ConfinementError::Unproven(
            "a granted read root was not readable inside the sandbox".to_string(),
        ));
    }

    for denied in [
        text(&workspace.denied)?.to_string(),
        backend.denied_system_file().to_string(),
    ] {
        let outcome = run_confined(
            backend,
            &workspace.root,
            PROBE_COMMAND,
            vec![denied.clone()],
        )
        .await?;
        if outcome.status.success() {
            return Err(ConfinementError::Unproven(format!(
                "{denied} was readable inside the sandbox"
            )));
        }
    }

    probe_network(backend, &workspace.root).await
}

async fn probe_network(backend: Backend, root: &Path) -> Result<(), ConfinementError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(probe_failure)?;
    let port = listener.local_addr().map_err(probe_failure)?.port();

    let connected = Arc::new(AtomicBool::new(false));
    let accepted = connected.clone();
    let acceptor = tokio::spawn(async move {
        if listener.accept().await.is_ok() {
            accepted.store(true, Ordering::SeqCst);
        }
    });

    let outcome = match network_probe_command(port) {
        Some((command, arguments)) => run_confined(backend, root, &command, arguments).await,
        None => Err(ConfinementError::Unproven(
            "no command able to open a TCP connection is installed".to_string(),
        )),
    };

    tokio::time::sleep(PROBE_SETTLE).await;
    acceptor.abort();

    let outcome = outcome?;
    if connected.load(Ordering::SeqCst) {
        return Err(ConfinementError::Unproven(
            "a confined command reached a local TCP listener".to_string(),
        ));
    }
    if outcome.status.success() {
        return Err(ConfinementError::Unproven(
            "a confined command reported a successful network connection".to_string(),
        ));
    }
    Ok(())
}

fn network_probe_command(port: u16) -> Option<(String, Vec<String>)> {
    let command = NETWORK_PROBE_COMMANDS
        .into_iter()
        .find(|candidate| executable_file(Path::new(candidate)).is_some())?;
    let arguments = if command.ends_with("curl") {
        vec![
            "--silent".to_string(),
            "--max-time".to_string(),
            "2".to_string(),
            format!("http://127.0.0.1:{port}/"),
        ]
    } else {
        vec![
            "-w".to_string(),
            "1".to_string(),
            "127.0.0.1".to_string(),
            port.to_string(),
        ]
    };
    Some((command.to_string(), arguments))
}

async fn run_confined(
    backend: Backend,
    root: &Path,
    command: &str,
    arguments: Vec<String>,
) -> Result<std::process::Output, ConfinementError> {
    let request = ConfinementRequest {
        read_roots: vec![root.to_path_buf()],
        write_roots: vec![root.to_path_buf()],
    };
    let invocation = Confinement::new(command, arguments, root)
        .with_roots(&request)
        .invocation(Some(backend))?;

    let run = Command::new(&invocation.program)
        .args(&invocation.arguments)
        .current_dir(root)
        .env_clear()
        .envs(&invocation.environment)
        .stdin(Stdio::null())
        .output();

    match timeout(PROBE_TIMEOUT, run).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(probe_failure(error)),
        Err(_) => Err(ConfinementError::Unproven(format!(
            "the sandbox probe did not finish within {}s",
            PROBE_TIMEOUT.as_secs()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_plain_path() {
        assert_eq!(escape("/tmp/work"), "\"/tmp/work\"");
    }

    #[test]
    fn test_escape_quotes_and_backslashes() {
        assert_eq!(escape("/tmp/a\"b\\c d"), "\"/tmp/a\\\"b\\\\c d\"");
    }

    #[test]
    fn test_escape_closing_backslash_cannot_escape_the_terminator() {
        let escaped = escape("/tmp/trailing\\");
        assert_eq!(escaped, "\"/tmp/trailing\\\\\"");
        assert_eq!(escaped.matches('"').count(), 2);
    }

    #[test]
    fn test_escape_preserves_unicode() {
        assert_eq!(escape("/tmp/日本語"), "\"/tmp/日本語\"");
    }

    #[test]
    fn test_text_rejects_relative_paths() {
        assert_eq!(
            text(Path::new("relative/path")),
            Err(ConfinementError::RelativePath("relative/path".to_string()))
        );
    }

    #[test]
    fn test_text_rejects_control_characters() {
        assert!(matches!(
            text(Path::new("/tmp/a\nb")),
            Err(ConfinementError::ControlCharacterInPath(_))
        ));
    }

    #[test]
    fn test_backend_executables() {
        assert_eq!(Backend::Seatbelt.executable(), SEATBELT);
        assert_eq!(Backend::Bubblewrap.executable(), BUBBLEWRAP);
    }

    #[test]
    fn test_command_path_prefixes_the_command_directory() {
        assert_eq!(
            command_path(Path::new("/opt/tools/bin/run")),
            "/opt/tools/bin:/usr/bin:/bin"
        );
    }

    #[test]
    fn test_command_path_does_not_repeat_system_entries() {
        assert_eq!(command_path(Path::new("/bin/cat")), "/bin:/usr/bin");
    }

    #[test]
    fn test_complete_environment_fills_defaults() {
        let environment = complete_environment(
            &BTreeMap::new(),
            Path::new("/bin/cat"),
            Path::new("/tmp/writable"),
        );
        assert_eq!(environment.get(HOME).unwrap(), "/tmp/writable");
        assert_eq!(environment.get(TMPDIR).unwrap(), "/tmp/writable");
        assert_eq!(environment.get(LC_ALL).unwrap(), LOCALE);
    }

    #[test]
    fn test_complete_environment_keeps_requested_values() {
        let requested = BTreeMap::from([(HOME.to_string(), "/tmp/mine".to_string())]);
        let environment = complete_environment(
            &requested,
            Path::new("/bin/cat"),
            Path::new("/tmp/writable"),
        );
        assert_eq!(environment.get(HOME).unwrap(), "/tmp/mine");
    }

    #[test]
    fn test_resolve_command_finds_absolute_path() {
        let resolved = resolve_command("/bin/cat", &BTreeMap::new()).unwrap();
        assert!(resolved.ends_with("cat"));
    }

    #[test]
    fn test_resolve_command_rejects_missing_command() {
        assert_eq!(
            resolve_command("/bin/definitely-not-a-command", &BTreeMap::new()),
            Err(ConfinementError::CommandNotFound(
                "/bin/definitely-not-a-command".to_string()
            ))
        );
    }

    #[test]
    fn test_resolve_command_searches_the_requested_path() {
        let environment = BTreeMap::from([(PATH.to_string(), "/bin".to_string())]);
        assert!(resolve_command("cat", &environment).is_ok());
    }

    #[test]
    fn test_unsupported_platform_never_produces_an_invocation() {
        let confinement = Confinement::new("/bin/cat", vec![], "/tmp");
        assert_eq!(
            confinement.invocation(None),
            Err(ConfinementError::UnsupportedPlatform)
        );
    }
}

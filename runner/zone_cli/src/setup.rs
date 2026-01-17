//! Setup module for Zone development environment
//!
//! Provides functionality to:
//! - Check prerequisites (docker, docker-compose, openssl)
//! - Generate secure secrets
//! - Create .env file from template
//! - Validate configuration

use std::path::PathBuf;
use std::process::Command;

use console::{Style, style};
use dialoguer::{Confirm, Select};
use rand::Rng;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SetupError {
    #[error("Missing prerequisites: {0}")]
    MissingPrerequisites(String),

    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("User cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, SetupError>;

/// Styles for console output
#[allow(dead_code)]
struct Styles {
    info: Style,
    warn: Style,
    error: Style,
    step: Style,
    success: Style,
}

impl Default for Styles {
    fn default() -> Self {
        Self {
            info: Style::new().green(),
            warn: Style::new().yellow(),
            error: Style::new().red(),
            step: Style::new().blue().bold(),
            success: Style::new().green().bold(),
        }
    }
}

/// Setup context with paths and styles
pub struct Setup {
    project_root: PathBuf,
    env_file: PathBuf,
    env_example: PathBuf,
    styles: Styles,
}

impl Setup {
    /// Create a new setup context for the given project root
    pub fn new(project_root: PathBuf) -> Self {
        let env_file = project_root.join(".env");
        let env_example = project_root.join(".env.example");

        Self {
            project_root,
            env_file,
            env_example,
            styles: Styles::default(),
        }
    }

    /// Run the interactive setup menu
    pub async fn run_interactive(&self) -> Result<()> {
        println!();
        println!(
            "{}",
            style("╔════════════════════════════════════════╗")
                .blue()
                .bold()
        );
        println!(
            "{}",
            style("║         Zone Setup                     ║")
                .blue()
                .bold()
        );
        println!(
            "{}",
            style("╚════════════════════════════════════════╝")
                .blue()
                .bold()
        );
        println!();

        let options = vec![
            "Full setup (recommended for first-time setup)",
            "Generate secrets only",
            "Validate configuration",
            "Exit",
        ];

        let selection = Select::new()
            .with_prompt("Select option")
            .items(&options)
            .default(0)
            .interact()
            .map_err(|_| SetupError::Cancelled)?;

        match selection {
            0 => {
                self.check_prerequisites()?;
                self.setup_env_file()?;
                self.validate_config()?;
                self.print_next_steps();
            }
            1 => {
                self.setup_env_file()?;
            }
            2 => {
                self.validate_config()?;
            }
            3 => {
                println!("{}", self.styles.info.apply_to("Exiting..."));
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    /// Run full setup non-interactively
    pub fn run_full(&self) -> Result<()> {
        self.check_prerequisites()?;
        self.setup_env_file()?;
        self.validate_config()?;
        self.print_next_steps();
        Ok(())
    }

    /// Check if all prerequisites are installed
    pub fn check_prerequisites(&self) -> Result<()> {
        self.log_step("Checking prerequisites...");

        let mut missing = Vec::new();

        if !command_exists("docker") {
            missing.push("docker");
        }

        if !command_exists("docker-compose") && !docker_compose_plugin_exists() {
            missing.push("docker-compose");
        }

        if !command_exists("openssl") {
            missing.push("openssl");
        }

        if !missing.is_empty() {
            return Err(SetupError::MissingPrerequisites(missing.join(", ")));
        }

        self.log_info("✓ All prerequisites met");
        Ok(())
    }

    /// Setup .env file from template and generate secrets
    pub fn setup_env_file(&self) -> Result<()> {
        self.log_step("Setting up .env file...");

        // Check if .env already exists
        if self.env_file.exists() {
            let overwrite = Confirm::new()
                .with_prompt(".env file already exists. Overwrite?")
                .default(false)
                .interact()
                .map_err(|_| SetupError::Cancelled)?;

            if !overwrite {
                self.log_info("Keeping existing .env file");
                return Ok(());
            }
        }

        // Check if .env.example exists
        if !self.env_example.exists() {
            return Err(SetupError::FileNotFound(self.env_example.clone()));
        }

        // Copy .env.example to .env
        self.log_info("Copying .env.example to .env...");
        std::fs::copy(&self.env_example, &self.env_file)?;

        // Generate secrets
        self.log_info("Generating secure secrets...");
        let litellm_key = generate_secret();
        let litellm_salt = generate_secret();
        let searxng_secret = generate_secret();
        let postgres_password = generate_secret();
        let manager_api_key = generate_secret();
        let jwt_secret = generate_secret();

        // Read and update .env file
        let mut content = std::fs::read_to_string(&self.env_file)?;

        content = replace_env_value(&content, "SECURITY_LITELLM_MASTER_KEY", &litellm_key);
        content = replace_env_value(&content, "SECURITY_LITELLM_SALT_KEY", &litellm_salt);
        content = replace_env_value(&content, "SECURITY_SEARXNG_SECRET_KEY", &searxng_secret);
        content = replace_env_value(&content, "POSTGRES_PASSWORD", &postgres_password);
        content = replace_env_value(&content, "SECURITY_MANAGER_API_KEY", &manager_api_key);
        content = replace_env_value(&content, "JWT_SECRET", &jwt_secret);
        content = replace_env_value(&content, "WEBUI_OPENAI_API_KEY", &litellm_key);

        // Update CORS origin based on domain
        if let Some(domain) = get_env_value(&content, "DOMAIN_HOST_WEBUI")
            && !domain.is_empty()
        {
            content = replace_env_value(
                &content,
                "WEBUI_CORS_ALLOW_ORIGIN",
                &format!("http://{}", domain),
            );
            self.log_info(&format!("Set WEBUI_CORS_ALLOW_ORIGIN to http://{}", domain));
        }

        std::fs::write(&self.env_file, content)?;

        self.log_info("✓ Secrets generated and inserted into .env");
        println!();
        self.log_warn("Review .env and update:");
        self.log_warn("  - Domain name (DOMAIN_HOST_WEBUI)");
        self.log_warn("  - VPN credentials (VPN_OPENVPN_USER, VPN_OPENVPN_PASSWORD)");
        self.log_warn("  - ACME email (ADVANCED_ACME_EMAIL)");
        self.log_warn("  - Model choices (OLLAMA_MODEL_*)");

        Ok(())
    }

    /// Validate the configuration
    pub fn validate_config(&self) -> Result<()> {
        self.log_step("Validating configuration...");

        if !self.env_file.exists() {
            return Err(SetupError::FileNotFound(self.env_file.clone()));
        }

        let content = std::fs::read_to_string(&self.env_file)?;

        // Check for insecure defaults
        if let Some(val) = get_env_value(&content, "SECURITY_LITELLM_MASTER_KEY")
            && val.contains("dev-insecure")
        {
            self.log_warn("SECURITY_LITELLM_MASTER_KEY is using default insecure value");
        }

        if let Some(val) = get_env_value(&content, "SECURITY_SEARXNG_SECRET_KEY")
            && val.contains("dev-insecure")
        {
            self.log_warn("SECURITY_SEARXNG_SECRET_KEY is using default insecure value");
        }

        // Check VPN configuration
        let vpn_type = get_env_value(&content, "VPN_TYPE").unwrap_or_default();
        if vpn_type == "openvpn" || vpn_type.is_empty() {
            if get_env_value(&content, "VPN_OPENVPN_USER")
                .unwrap_or_default()
                .is_empty()
            {
                self.log_warn("VPN_OPENVPN_USER not set (OpenVPN will not work)");
            }
            if get_env_value(&content, "VPN_OPENVPN_PASSWORD")
                .unwrap_or_default()
                .is_empty()
            {
                self.log_warn("VPN_OPENVPN_PASSWORD not set (OpenVPN will not work)");
            }
        } else if vpn_type == "wireguard" {
            if get_env_value(&content, "VPN_WIREGUARD_PRIVATE_KEY")
                .unwrap_or_default()
                .is_empty()
            {
                self.log_warn("VPN_WIREGUARD_PRIVATE_KEY not set (WireGuard will not work)");
            }
            if get_env_value(&content, "VPN_WIREGUARD_ADDRESSES")
                .unwrap_or_default()
                .is_empty()
            {
                self.log_warn("VPN_WIREGUARD_ADDRESSES not set (WireGuard will not work)");
            }
        }

        // Validate docker compose config
        if docker_compose_plugin_exists() || command_exists("docker-compose") {
            self.log_info("Validating Docker Compose configuration...");
            let status = if docker_compose_plugin_exists() {
                Command::new("docker")
                    .args(["compose", "config", "--quiet"])
                    .current_dir(&self.project_root)
                    .status()
            } else {
                Command::new("docker-compose")
                    .args(["config", "--quiet"])
                    .current_dir(&self.project_root)
                    .status()
            };

            match status {
                Ok(s) if s.success() => {
                    self.log_info("✓ Docker Compose configuration is valid");
                }
                _ => {
                    self.log_warn("Docker Compose configuration may have issues");
                }
            }
        }

        self.log_info("✓ Configuration looks good!");

        Ok(())
    }

    /// Print next steps after setup
    fn print_next_steps(&self) {
        println!();
        println!(
            "{}",
            style("═══════════════════════════════════════════════════════════")
                .green()
                .bold()
        );
        println!("{}", style("Setup Complete!").green().bold());
        println!(
            "{}",
            style("═══════════════════════════════════════════════════════════")
                .green()
                .bold()
        );
        println!();
        println!("Next steps:");
        println!();
        println!("  1. Review your .env file:");
        println!(
            "     {}",
            style(format!("nano {}", self.env_file.display())).blue()
        );
        println!();
        println!("  2. Update VPN credentials (if using VPN):");
        println!(
            "     {} and {}",
            style("VPN_OPENVPN_USER").blue(),
            style("VPN_OPENVPN_PASSWORD").blue()
        );
        println!();
        println!("  3. Update domain name for your setup:");
        println!("     {}", style("DOMAIN_HOST_WEBUI").blue());
        println!();
        println!("  4. Start the stack:");
        println!(
            "     {} or {}",
            style("make up").blue(),
            style("docker compose up -d").blue()
        );
        println!();
        println!("  5. Check logs:");
        println!(
            "     {} or {}",
            style("make logs").blue(),
            style("docker compose logs -f").blue()
        );
        println!();
        println!("  6. Access the web UI:");
        println!("     {}", style("https://webui.localhost").blue());
        println!();
        println!(
            "{}",
            style("═══════════════════════════════════════════════════════════")
                .green()
                .bold()
        );
        println!();
    }

    fn log_info(&self, msg: &str) {
        println!("{} {}", self.styles.info.apply_to("[INFO]"), msg);
    }

    fn log_warn(&self, msg: &str) {
        println!("{} {}", self.styles.warn.apply_to("[WARN]"), msg);
    }

    fn log_step(&self, msg: &str) {
        println!();
        println!("{} {}", self.styles.step.apply_to("▶"), msg);
        println!();
    }
}

/// Check if a command exists in PATH
fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if docker compose plugin exists
fn docker_compose_plugin_exists() -> bool {
    Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a secure random base64 secret
fn generate_secret() -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill(&mut bytes);
    STANDARD.encode(bytes)
}

/// Get value from .env content
fn get_env_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            let v = v.trim();
            // Remove quotes
            let v = v.trim_matches('"').trim_matches('\'');
            return Some(v.to_string());
        }
    }
    None
}

/// Replace value in .env content
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

    // If key wasn't found, append it
    if !found {
        result.push_str(&format!("{}={}\n", key, value));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_value_simple() {
        let content = "KEY=value\nOTHER=other";
        assert_eq!(get_env_value(content, "KEY"), Some("value".to_string()));
        assert_eq!(get_env_value(content, "OTHER"), Some("other".to_string()));
    }

    #[test]
    fn test_get_env_value_quoted() {
        let content = "KEY=\"quoted value\"\nSINGLE='single quoted'";
        assert_eq!(
            get_env_value(content, "KEY"),
            Some("quoted value".to_string())
        );
        assert_eq!(
            get_env_value(content, "SINGLE"),
            Some("single quoted".to_string())
        );
    }

    #[test]
    fn test_get_env_value_with_comments() {
        let content = "# comment\nKEY=value\n# another comment";
        assert_eq!(get_env_value(content, "KEY"), Some("value".to_string()));
    }

    #[test]
    fn test_get_env_value_not_found() {
        let content = "KEY=value";
        assert_eq!(get_env_value(content, "MISSING"), None);
    }

    #[test]
    fn test_get_env_value_empty() {
        let content = "KEY=\nOTHER=value";
        assert_eq!(get_env_value(content, "KEY"), Some("".to_string()));
    }

    #[test]
    fn test_get_env_value_with_spaces() {
        let content = "  KEY  =  value  ";
        assert_eq!(get_env_value(content, "KEY"), Some("value".to_string()));
    }

    #[test]
    fn test_replace_env_value_existing() {
        let content = "KEY=old\nOTHER=other";
        let result = replace_env_value(content, "KEY", "new");
        assert!(result.contains("KEY=new"));
        assert!(result.contains("OTHER=other"));
        assert!(!result.contains("old"));
    }

    #[test]
    fn test_replace_env_value_new_key() {
        let content = "EXISTING=value";
        let result = replace_env_value(content, "NEW", "new_value");
        assert!(result.contains("EXISTING=value"));
        assert!(result.contains("NEW=new_value"));
    }

    #[test]
    fn test_replace_env_value_preserves_comments() {
        let content = "# comment\nKEY=old\n# another";
        let result = replace_env_value(content, "KEY", "new");
        assert!(result.contains("# comment"));
        assert!(result.contains("# another"));
    }

    #[test]
    fn test_replace_env_value_empty_content() {
        let content = "";
        let result = replace_env_value(content, "KEY", "value");
        assert!(result.contains("KEY=value"));
    }

    #[test]
    fn test_generate_secret_length() {
        let secret = generate_secret();
        // Base64 encoded 32 bytes = 44 characters (with padding)
        assert!(secret.len() >= 40);
    }

    #[test]
    fn test_generate_secret_uniqueness() {
        let secret1 = generate_secret();
        let secret2 = generate_secret();
        assert_ne!(secret1, secret2);
    }

    #[test]
    fn test_setup_error_display() {
        let err = SetupError::MissingPrerequisites("docker".to_string());
        assert!(err.to_string().contains("docker"));

        let err = SetupError::Cancelled;
        assert_eq!(err.to_string(), "User cancelled");
    }

    #[test]
    fn test_setup_new() {
        let setup = Setup::new(PathBuf::from("/tmp/test"));
        assert_eq!(setup.project_root, PathBuf::from("/tmp/test"));
        assert_eq!(setup.env_file, PathBuf::from("/tmp/test/.env"));
        assert_eq!(setup.env_example, PathBuf::from("/tmp/test/.env.example"));
    }

    #[test]
    fn test_styles_default() {
        let styles = Styles::default();
        // Just verify it doesn't panic
        let _ = format!("{:?}", styles.info);
    }
}

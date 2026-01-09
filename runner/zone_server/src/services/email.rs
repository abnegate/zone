//! Email service for sending verification and password reset emails
//!
//! Supports SMTP configuration via environment variables.

use lettre::address::AddressError;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::env;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("Failed to build email: {0}")]
    BuildError(#[from] lettre::error::Error),

    #[error("Failed to send email: {0}")]
    SendError(#[from] lettre::transport::smtp::Error),

    #[error("Invalid email address: {0}")]
    AddressError(#[from] AddressError),

    #[error("Email service not configured")]
    NotConfigured,

    #[error("Invalid email configuration: {0}")]
    InvalidConfig(String),
}

pub type EmailResult<T> = Result<T, EmailError>;

/// Email service configuration
#[derive(Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub from_email: String,
    pub from_name: String,
}

/// Custom Debug implementation that redacts the SMTP password
impl std::fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailConfig")
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_user", &self.smtp_user)
            .field("smtp_password", &"[REDACTED]")
            .field("from_email", &self.from_email)
            .field("from_name", &self.from_name)
            .finish()
    }
}

impl EmailConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> EmailResult<Self> {
        let smtp_host = env::var("SMTP_HOST").map_err(|_| EmailError::NotConfigured)?;

        let smtp_port = env::var("SMTP_PORT")
            .unwrap_or_else(|_| "587".to_string())
            .parse::<u16>()
            .map_err(|e| EmailError::InvalidConfig(format!("Invalid SMTP_PORT: {}", e)))?;

        let smtp_user = env::var("SMTP_USER").map_err(|_| EmailError::NotConfigured)?;

        let smtp_password = env::var("SMTP_PASSWORD").map_err(|_| EmailError::NotConfigured)?;

        let from_email = env::var("SMTP_FROM").unwrap_or_else(|_| "noreply@zone.app".to_string());

        let from_name = env::var("SMTP_FROM_NAME").unwrap_or_else(|_| "Zone".to_string());

        Ok(Self {
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_password,
            from_email,
            from_name,
        })
    }
}

/// Email service for sending transactional emails
pub struct EmailService {
    config: EmailConfig,
    mailer: SmtpTransport,
}

impl EmailService {
    /// Create a new email service with the given configuration
    pub fn new(config: EmailConfig) -> EmailResult<Self> {
        let creds = Credentials::new(config.smtp_user.clone(), config.smtp_password.clone());

        let mailer = SmtpTransport::relay(&config.smtp_host)
            .map_err(|e| EmailError::InvalidConfig(format!("Invalid SMTP host: {}", e)))?
            .port(config.smtp_port)
            .credentials(creds)
            .build();

        Ok(Self { config, mailer })
    }

    /// Create email service from environment variables
    pub fn from_env() -> EmailResult<Self> {
        let config = EmailConfig::from_env()?;
        Self::new(config)
    }

    /// Send an email verification email
    pub async fn send_verification_email(
        &self,
        to_email: &str,
        to_name: &str,
        verification_url: &str,
    ) -> EmailResult<()> {
        let subject = "Verify your email address";
        let body = format!(
            r#"
Hello {},

Thank you for signing up for Zone!

Please verify your email address by clicking the link below:

{}

This link will expire in 24 hours.

If you did not create an account, you can safely ignore this email.

Best regards,
The Zone Team
            "#,
            to_name, verification_url
        );

        self.send_email(to_email, subject, &body).await
    }

    /// Send a password reset email
    pub async fn send_password_reset_email(
        &self,
        to_email: &str,
        to_name: &str,
        reset_url: &str,
    ) -> EmailResult<()> {
        let subject = "Reset your password";
        let body = format!(
            r#"
Hello {},

We received a request to reset your password for your Zone account.

Click the link below to reset your password:

{}

This link will expire in 1 hour.

If you did not request a password reset, you can safely ignore this email.
Your password will not be changed unless you click the link above and create a new password.

Best regards,
The Zone Team
            "#,
            to_name, reset_url
        );

        self.send_email(to_email, subject, &body).await
    }

    /// Send an invitation email
    pub async fn send_invitation_email(
        &self,
        to_email: &str,
        org_name: &str,
        inviter_name: &str,
        invitation_url: &str,
    ) -> EmailResult<()> {
        let subject = format!("You've been invited to join {} on Zone", org_name);
        let body = format!(
            r#"
Hello,

{} has invited you to join the {} organization on Zone.

Click the link below to accept the invitation:

{}

This link will expire in 7 days.

If you don't have a Zone account yet, you'll be able to create one after clicking the link.

Best regards,
The Zone Team
            "#,
            inviter_name, org_name, invitation_url
        );

        self.send_email(to_email, &subject, &body).await
    }

    /// Send a generic email
    async fn send_email(&self, to_email: &str, subject: &str, body: &str) -> EmailResult<()> {
        let email = Message::builder()
            .from(format!("{} <{}>", self.config.from_name, self.config.from_email).parse()?)
            .to(to_email.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())?;

        // Send email in a blocking task to avoid blocking the async runtime
        let mailer = self.mailer.clone();
        tokio::task::spawn_blocking(move || mailer.send(&email))
            .await
            .map_err(|e| EmailError::InvalidConfig(format!("Task join error: {}", e)))?
            .map_err(EmailError::SendError)?;

        Ok(())
    }
}

/// Mock email service for testing (doesn't actually send emails)
#[cfg(test)]
pub struct MockEmailService {
    pub sent_emails: std::sync::Arc<tokio::sync::Mutex<Vec<SentEmail>>>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct SentEmail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[cfg(test)]
impl Default for MockEmailService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockEmailService {
    pub fn new() -> Self {
        Self {
            sent_emails: std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }

    pub async fn send_verification_email(
        &self,
        to_email: &str,
        _to_name: &str,
        _verification_url: &str,
    ) -> EmailResult<()> {
        let mut emails = self.sent_emails.lock().await;
        emails.push(SentEmail {
            to: to_email.to_string(),
            subject: "Verify your email address".to_string(),
            body: "Verification email".to_string(),
        });
        Ok(())
    }

    pub async fn send_password_reset_email(
        &self,
        to_email: &str,
        _to_name: &str,
        _reset_url: &str,
    ) -> EmailResult<()> {
        let mut emails = self.sent_emails.lock().await;
        emails.push(SentEmail {
            to: to_email.to_string(),
            subject: "Reset your password".to_string(),
            body: "Password reset email".to_string(),
        });
        Ok(())
    }

    pub async fn get_sent_emails(&self) -> Vec<SentEmail> {
        self.sent_emails.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.sent_emails.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_email_service_tracks_sent_emails() {
        let service = MockEmailService::new();

        service
            .send_verification_email("test@example.com", "Test User", "http://example.com/verify")
            .await
            .expect("Failed to send verification email");

        service
            .send_password_reset_email("test@example.com", "Test User", "http://example.com/reset")
            .await
            .expect("Failed to send reset email");

        let emails = service.get_sent_emails().await;
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0].to, "test@example.com");
        assert_eq!(emails[0].subject, "Verify your email address");
        assert_eq!(emails[1].subject, "Reset your password");
    }

    #[tokio::test]
    async fn test_mock_email_service_can_be_cleared() {
        let service = MockEmailService::new();

        service
            .send_verification_email("test@example.com", "Test User", "http://example.com/verify")
            .await
            .expect("Failed to send email");

        assert_eq!(service.get_sent_emails().await.len(), 1);

        service.clear().await;

        assert_eq!(service.get_sent_emails().await.len(), 0);
    }
}

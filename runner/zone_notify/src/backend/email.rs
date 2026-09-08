//! Email, over SMTP.

use std::fmt;

use async_trait::async_trait;
use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use zone_core::tools::sanitize;

use crate::backend::smtp::SmtpConfig;
use crate::channel::Channel;
use crate::error::NotifyError;
use crate::notification::Notification;
use crate::notifier::Notifier;

/// Delivers to a fixed set of recipients through one SMTP relay.
///
/// This is a separate transport from `zone_email`, which owns the account
/// flow's three templates and keeps its own synchronous relay. That crate's
/// generic send is private, so a notification of arbitrary content cannot go
/// through it without changing it; see this crate's documentation for what
/// folding the two together would involve.
///
/// Construct and drop this inside a Tokio runtime. When `lettre`'s `pool`
/// feature is on anywhere in the build, its connection pool spawns a task
/// from its own `Drop`, and a panic in a destructor aborts the process rather
/// than unwinding.
pub struct Email {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    recipients: Vec<Mailbox>,
    host: String,
    name: Option<String>,
}

impl Email {
    /// Connect to the relay in `config` and deliver to `recipients`.
    pub fn new(config: SmtpConfig, recipients: &[&str]) -> Result<Self, NotifyError> {
        if recipients.is_empty() {
            return Err(NotifyError::Malformed {
                message: "an email channel needs at least one recipient".to_string(),
            });
        }

        let from = mailbox(&config.from_address, Some(config.from_name.clone()))?;
        let recipients = recipients
            .iter()
            .map(|recipient| mailbox(recipient, None))
            .collect::<Result<Vec<Mailbox>, NotifyError>>()?;

        let credentials =
            Credentials::new(config.user.clone(), config.password.expose().to_string());
        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
            .map_err(|error| NotifyError::Smtp {
                host: config.host.clone(),
                message: describe(error),
            })?
            .port(config.port)
            .credentials(credentials)
            .build();

        Ok(Self {
            transport,
            from,
            recipients,
            host: config.host,
            name: None,
        })
    }

    /// Label this instance, for a workspace with more than one recipient set.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    fn compose(&self, notification: &Notification) -> Result<Message, NotifyError> {
        let mut builder = Message::builder()
            .from(self.from.clone())
            .subject(notification.title());

        for recipient in &self.recipients {
            builder = builder.to(recipient.clone());
        }

        builder
            .header(ContentType::TEXT_PLAIN)
            .body(notification.to_plain_text())
            .map_err(|error| NotifyError::Malformed {
                message: error.to_string(),
            })
    }
}

#[async_trait]
impl Notifier for Email {
    fn channel(&self) -> Channel {
        Channel::EMAIL
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    async fn deliver(&self, notification: &Notification) -> Result<(), NotifyError> {
        let message = self.compose(notification)?;
        self.transport
            .send(message)
            .await
            .map_err(|error| NotifyError::Smtp {
                host: self.host.clone(),
                message: describe(error),
            })?;
        Ok(())
    }
}

/// Written out rather than derived: the transport holds the relay password
/// inside a `lettre::Credentials`, which has no redacting `Debug` of its own.
impl fmt::Debug for Email {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Email")
            .field("host", &self.host)
            .field("from", &self.from)
            .field("recipients", &self.recipients)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

fn mailbox(address: &str, name: Option<String>) -> Result<Mailbox, NotifyError> {
    let parsed: Address = address.parse().map_err(|_| NotifyError::Malformed {
        message: format!("{address} is not a valid email address"),
    })?;
    Ok(Mailbox::new(name, parsed))
}

/// An SMTP error's message, sanitized.
///
/// A relay's reply text is written by the far end, so it is treated like any
/// other outside text on its way to a log line.
fn describe(error: lettre::transport::smtp::Error) -> String {
    sanitize(&error.to_string()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::severity::Severity;
    use zone_core::SecretValue;

    fn config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.zone.test".to_string(),
            port: 587,
            user: "postmaster".to_string(),
            password: SecretValue::new("hunter2-not-a-real-password"),
            from_address: "noreply@zone.test".to_string(),
            from_name: "Zone".to_string(),
        }
    }

    fn email() -> Email {
        Email::new(config(), &["sam@example.test"]).expect("configured")
    }

    fn rendered(notification: &Notification) -> String {
        String::from_utf8(email().compose(notification).expect("composed").formatted())
            .expect("utf-8")
    }

    #[test]
    fn a_channel_with_no_recipients_is_refused() {
        let error = Email::new(config(), &[]).expect_err("no recipients");
        assert!(matches!(error, NotifyError::Malformed { .. }));
        assert!(error.to_string().contains("at least one recipient"));
    }

    #[test]
    fn an_unparseable_recipient_is_refused() {
        let error = Email::new(config(), &["not an address"]).expect_err("bad recipient");
        assert!(matches!(error, NotifyError::Malformed { .. }));
    }

    #[test]
    fn an_unparseable_sender_is_refused() {
        let mut broken = config();
        broken.from_address = "@@@".to_string();
        assert!(matches!(
            Email::new(broken, &["sam@example.test"]).expect_err("bad sender"),
            NotifyError::Malformed { .. }
        ));
    }

    #[tokio::test]
    async fn the_subject_is_the_title_and_the_body_carries_everything() {
        let message = rendered(
            &Notification::new("Build failed", "3 tests failed")
                .severity(Severity::Error)
                .field("Branch", "main")
                .link("https://zone.test/b/1"),
        );

        assert!(message.contains("Subject: Build failed"));
        assert!(message.contains("From: Zone <noreply@zone.test>"));
        assert!(message.contains("To: sam@example.test"));
        assert!(message.contains("Content-Type: text/plain"));
        assert!(message.contains("3 tests failed"));
        assert!(message.contains("Branch: main"));
        assert!(message.contains("https://zone.test/b/1"));
    }

    #[tokio::test]
    async fn every_recipient_is_addressed() {
        let email =
            Email::new(config(), &["sam@example.test", "alex@example.test"]).expect("configured");
        let message = String::from_utf8(
            email
                .compose(&Notification::new("Title", "Body"))
                .expect("composed")
                .formatted(),
        )
        .expect("utf-8");

        assert!(message.contains("sam@example.test"));
        assert!(message.contains("alex@example.test"));
    }

    #[tokio::test]
    async fn the_relay_password_never_reaches_the_message() {
        let message = rendered(&Notification::new("Title", "Body"));
        assert!(!message.contains("hunter2"), "leaked into the message");
    }

    #[tokio::test]
    async fn a_credential_in_the_body_is_redacted_before_it_is_composed() {
        let message = rendered(&Notification::new(
            "Deploy log",
            "GITHUB_TOKEN=ghp_0123456789abcdefghij",
        ));
        assert!(!message.contains("ghp_0123456789abcdefghij"));
        assert!(message.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn the_relay_password_never_appears_in_debug() {
        let rendered = format!("{:?}", email());
        assert!(!rendered.contains("hunter2"), "leaked: {rendered}");
        assert!(rendered.contains("smtp.zone.test"));
    }

    #[tokio::test]
    async fn the_channel_and_name_are_reported() {
        assert_eq!(email().channel(), Channel::EMAIL);
        assert_eq!(email().name(), None);
        assert_eq!(email().named("ops").name(), Some("ops"));
    }
}

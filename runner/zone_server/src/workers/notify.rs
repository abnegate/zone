//! Where a worker's outbound message goes.
//!
//! Both the regression watch and the scheduled digest have something to say and
//! no opinion about who hears it, so they share one [`Fanout`] built here from
//! the process environment. A channel that is not configured is simply absent:
//! a workspace running without Slack has not failed at anything, and
//! [`Fanout::deliver`] reports an empty fan-out rather than an error.
//!
//! Resolution is split from reading the environment so every parsing rule is
//! testable without touching the process.

use std::sync::Arc;
use std::time::Duration;

use zone_core::SecretValue;
use zone_notify::{Discord, Email, Fanout, Notifier, NotifyError, Slack, SmtpConfig};

const SLACK_VARIABLE: &str = "ZONE_NOTIFY_SLACK_WEBHOOK";
const DISCORD_VARIABLE: &str = "ZONE_NOTIFY_DISCORD_WEBHOOK";
const EMAIL_TO_VARIABLE: &str = "ZONE_NOTIFY_EMAIL_TO";
const TIMEOUT_VARIABLE: &str = "ZONE_NOTIFY_TIMEOUT_SECONDS";

const SMTP_HOST_VARIABLE: &str = "SMTP_HOST";
const SMTP_PORT_VARIABLE: &str = "SMTP_PORT";
const SMTP_USER_VARIABLE: &str = "SMTP_USER";
const SMTP_PASSWORD_VARIABLE: &str = "SMTP_PASSWORD";
const SMTP_FROM_VARIABLE: &str = "SMTP_FROM";
const SMTP_FROM_NAME_VARIABLE: &str = "SMTP_FROM_NAME";

const DEFAULT_SMTP_PORT: u16 = 587;
const DEFAULT_FROM_NAME: &str = "Zone";
const DEFAULT_TIMEOUT_SECONDS: u64 = 10;
const MAXIMUM_TIMEOUT_SECONDS: u64 = 120;

/// The raw environment a set of channels is resolved from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotifyEnvironment {
    pub slack_webhook: Option<String>,
    pub discord_webhook: Option<String>,
    pub email_recipients: Option<String>,
    pub timeout_seconds: Option<String>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<String>,
    pub smtp_user: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from: Option<String>,
    pub smtp_from_name: Option<String>,
}

impl NotifyEnvironment {
    pub fn from_process() -> Self {
        Self {
            slack_webhook: std::env::var(SLACK_VARIABLE).ok(),
            discord_webhook: std::env::var(DISCORD_VARIABLE).ok(),
            email_recipients: std::env::var(EMAIL_TO_VARIABLE).ok(),
            timeout_seconds: std::env::var(TIMEOUT_VARIABLE).ok(),
            smtp_host: std::env::var(SMTP_HOST_VARIABLE).ok(),
            smtp_port: std::env::var(SMTP_PORT_VARIABLE).ok(),
            smtp_user: std::env::var(SMTP_USER_VARIABLE).ok(),
            smtp_password: std::env::var(SMTP_PASSWORD_VARIABLE).ok(),
            smtp_from: std::env::var(SMTP_FROM_VARIABLE).ok(),
            smtp_from_name: std::env::var(SMTP_FROM_NAME_VARIABLE).ok(),
        }
    }
}

/// The channels a worker will deliver to, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifySettings {
    pub slack_webhook: Option<String>,
    pub discord_webhook: Option<String>,
    pub email_recipients: Vec<String>,
    pub timeout: Duration,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            slack_webhook: None,
            discord_webhook: None,
            email_recipients: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
        }
    }
}

impl NotifySettings {
    pub fn resolve(environment: &NotifyEnvironment) -> Self {
        Self {
            slack_webhook: trimmed(environment.slack_webhook.as_deref()),
            discord_webhook: trimmed(environment.discord_webhook.as_deref()),
            email_recipients: recipients(environment.email_recipients.as_deref()),
            timeout: timeout(environment.timeout_seconds.as_deref()),
        }
    }

    /// Whether any channel at all was configured.
    pub fn is_configured(&self) -> bool {
        self.slack_webhook.is_some()
            || self.discord_webhook.is_some()
            || !self.email_recipients.is_empty()
    }
}

fn trimmed(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn recipients(raw: Option<&str>) -> Vec<String> {
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|address| !address.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn timeout(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds.min(MAXIMUM_TIMEOUT_SECONDS))
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
}

fn smtp(environment: &NotifyEnvironment) -> Option<SmtpConfig> {
    Some(SmtpConfig {
        host: trimmed(environment.smtp_host.as_deref())?,
        port: environment
            .smtp_port
            .as_deref()
            .and_then(|value| match value.trim().parse() {
                Ok(port) => Some(port),
                Err(_) => {
                    tracing::warn!(
                        value,
                        default = DEFAULT_SMTP_PORT,
                        "SMTP_PORT could not be read; every send will fail against the default \
                         port and retry forever with no log naming the cause"
                    );
                    None
                }
            })
            .unwrap_or(DEFAULT_SMTP_PORT),
        user: trimmed(environment.smtp_user.as_deref())?,
        password: SecretValue::new(environment.smtp_password.clone()?),
        from_address: trimmed(environment.smtp_from.as_deref())?,
        from_name: trimmed(environment.smtp_from_name.as_deref())
            .unwrap_or_else(|| DEFAULT_FROM_NAME.to_string()),
    })
}

/// The channels these settings describe, each already validated.
///
/// A channel that will not construct is logged and left out rather than failing
/// the build: one malformed webhook should cost that channel, not every other
/// one configured beside it.
///
/// Call this from inside the tokio runtime. The email backend builds a pooled
/// SMTP transport, which needs a reactor to attach its timers to and panics
/// without one.
pub fn channels(environment: &NotifyEnvironment) -> Vec<Arc<dyn Notifier>> {
    let settings = NotifySettings::resolve(environment);
    let mut channels: Vec<Arc<dyn Notifier>> = Vec::new();

    if let Some(webhook) = &settings.slack_webhook {
        match Slack::new(webhook) {
            Ok(slack) => channels.push(Arc::new(slack)),
            Err(error) => tracing::warn!(%error, "Slack notification channel is misconfigured"),
        }
    }

    if let Some(webhook) = &settings.discord_webhook {
        match Discord::new(webhook) {
            Ok(discord) => channels.push(Arc::new(discord)),
            Err(error) => tracing::warn!(%error, "Discord notification channel is misconfigured"),
        }
    }

    if !settings.email_recipients.is_empty() {
        match smtp(environment) {
            Some(config) => {
                let addresses: Vec<&str> = settings
                    .email_recipients
                    .iter()
                    .map(String::as_str)
                    .collect();
                match Email::new(config, &addresses) {
                    Ok(email) => channels.push(Arc::new(email)),
                    Err(error) => {
                        tracing::warn!(%error, "Email notification channel is misconfigured")
                    }
                }
            }
            None => tracing::warn!(
                "Email recipients are configured but the SMTP relay is not; skipping email"
            ),
        }
    }

    channels
}

/// A fan-out over `channels`, plus one more when a caller has a destination of
/// its own -- the chat an auto project reports into, say -- that the process
/// environment knows nothing about.
pub fn fanout_with(
    channels: Vec<Arc<dyn Notifier>>,
    extra: Option<Arc<dyn Notifier>>,
    timeout: Duration,
) -> Fanout {
    let mut fanout = Fanout::new().timeout(timeout);
    for channel in channels.into_iter().chain(extra) {
        fanout.register(channel);
    }
    fanout
}

/// Build the fan-out these settings describe.
pub fn fanout(environment: &NotifyEnvironment) -> Fanout {
    let timeout = NotifySettings::resolve(environment).timeout;
    fanout_with(channels(environment), None, timeout)
}

/// A workspace chat as a delivery destination.
///
/// The one channel that needs no configuration: a notice about a project lands
/// in that project's updates chat as an assistant message, stored like any
/// other and pushed to whoever has the chat open, so a person who set nothing
/// up still sees what automation did. Delivery is the insert; a message that
/// could not be stored was not delivered, and the fan-out reports it so.
pub struct ChatNotifier {
    pool: sqlx::PgPool,
    chat_id: uuid::Uuid,
    kind: &'static str,
}

impl ChatNotifier {
    /// A notifier that posts into one chat as the assistant.
    pub fn new(pool: sqlx::PgPool, chat_id: uuid::Uuid, kind: &'static str) -> Self {
        Self {
            pool,
            chat_id,
            kind,
        }
    }
}

#[async_trait::async_trait]
impl Notifier for ChatNotifier {
    /// The custom `chat` channel.
    fn channel(&self) -> zone_notify::Channel {
        zone_notify::Channel::custom("chat")
    }

    /// Insert the notification as an assistant message and publish it to the open chat.
    async fn deliver(&self, notification: &zone_notify::Notification) -> Result<(), NotifyError> {
        let failed = |error: sqlx::Error| NotifyError::Unreachable {
            host: "database".to_string(),
            message: error.to_string(),
        };
        let mut transaction = self.pool.begin().await.map_err(failed)?;
        let stored: serde_json::Value = sqlx::query_scalar(
            "INSERT INTO messages (chat_id, role, content, metadata) VALUES ($1, 'assistant', $2, $3) \
             RETURNING to_jsonb(messages.*)",
        )
        .bind(self.chat_id)
        .bind(notification.to_plain_text())
        .bind(serde_json::json!({
            "source": "auto_project",
            "kind": self.kind,
            "severity": notification.kind().to_string(),
            "link": notification.url(),
        }))
        .fetch_one(&mut *transaction)
        .await
        .map_err(failed)?;
        sqlx::query("UPDATE chats SET updated_at = NOW() WHERE id = $1")
            .bind(self.chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(failed)?;
        transaction.commit().await.map_err(failed)?;
        crate::db::actions::publish(self.chat_id, stored);
        Ok(())
    }
}

/// Build the fan-out the process environment describes.
pub fn from_process_environment() -> Fanout {
    fanout(&NotifyEnvironment::from_process())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(recipients: Option<&str>) -> NotifyEnvironment {
        NotifyEnvironment {
            email_recipients: recipients.map(str::to_string),
            ..NotifyEnvironment::default()
        }
    }

    #[test]
    fn nothing_configured_is_no_channels_rather_than_an_error() {
        let settings = NotifySettings::resolve(&NotifyEnvironment::default());

        assert!(!settings.is_configured());
        assert!(fanout(&NotifyEnvironment::default()).is_empty());
    }

    #[test]
    fn blank_values_are_treated_as_unset() {
        let settings = NotifySettings::resolve(&NotifyEnvironment {
            slack_webhook: Some("   ".to_string()),
            discord_webhook: Some(String::new()),
            ..NotifyEnvironment::default()
        });

        assert_eq!(settings.slack_webhook, None);
        assert_eq!(settings.discord_webhook, None);
    }

    #[test]
    fn recipients_are_split_and_trimmed() {
        let settings = NotifySettings::resolve(&environment(Some(
            " alerts@zone.test , ops@zone.test ,, oncall@zone.test ",
        )));

        assert_eq!(
            settings.email_recipients,
            vec!["alerts@zone.test", "ops@zone.test", "oncall@zone.test"]
        );
        assert!(settings.is_configured());
    }

    #[test]
    fn the_timeout_falls_back_and_is_capped() {
        assert_eq!(timeout(None), Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
        assert_eq!(
            timeout(Some("0")),
            Duration::from_secs(DEFAULT_TIMEOUT_SECONDS)
        );
        assert_eq!(
            timeout(Some("soon")),
            Duration::from_secs(DEFAULT_TIMEOUT_SECONDS)
        );
        assert_eq!(timeout(Some(" 30 ")), Duration::from_secs(30));
        assert_eq!(
            timeout(Some("9999")),
            Duration::from_secs(MAXIMUM_TIMEOUT_SECONDS),
            "one channel cannot hold the whole fan-out open indefinitely"
        );
    }

    #[test]
    fn a_malformed_webhook_costs_only_its_own_channel() {
        let built = fanout(&NotifyEnvironment {
            slack_webhook: Some("not-a-url".to_string()),
            discord_webhook: Some("https://discord.com/api/webhooks/1/token".to_string()),
            ..NotifyEnvironment::default()
        });

        assert_eq!(built.len(), 1, "Discord still registered");
    }

    #[test]
    fn a_webhook_pointing_somewhere_else_is_refused() {
        let built = fanout(&NotifyEnvironment {
            slack_webhook: Some("https://evil.test/services/T/B/x".to_string()),
            ..NotifyEnvironment::default()
        });

        assert!(built.is_empty(), "a Slack channel must point at Slack");
    }

    #[test]
    fn email_without_a_relay_is_skipped_rather_than_half_built() {
        let built = fanout(&environment(Some("alerts@zone.test")));

        assert!(built.is_empty());
    }

    #[tokio::test]
    async fn a_complete_relay_registers_the_email_channel() {
        let built = fanout(&NotifyEnvironment {
            email_recipients: Some("alerts@zone.test".to_string()),
            smtp_host: Some("smtp.zone.test".to_string()),
            smtp_user: Some("zone".to_string()),
            smtp_password: Some("secret".to_string()),
            smtp_from: Some("noreply@zone.test".to_string()),
            ..NotifyEnvironment::default()
        });

        assert_eq!(built.len(), 1);
    }

    #[test]
    fn a_relay_missing_its_sender_leaves_email_out() {
        assert!(
            smtp(&NotifyEnvironment {
                smtp_host: Some("smtp.zone.test".to_string()),
                smtp_user: Some("zone".to_string()),
                smtp_password: Some("secret".to_string()),
                ..NotifyEnvironment::default()
            })
            .is_none()
        );
    }
}

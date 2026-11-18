use std::fmt;
use std::time::Duration;

use abnegate_notify::{Sender, SmtpConfig, SmtpSecurity};
use async_trait::async_trait;
use lettre::message::header::{ContentTransferEncoding, ContentType};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::mail::{Letter, MailError, Outbox};

/// Polar gives up on a webhook after 10 seconds, so the whole send has to
/// finish, or fail, before then.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(7);

const UNSUPPORTED_SECURITY: &str = "the connection security is not supported";

pub struct Relay {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    host: String,
}

impl Relay {
    pub fn new(config: &SmtpConfig) -> Result<Self, MailError> {
        let from = mailbox(&config.sender)?;
        let builder = match config.effective_security() {
            SmtpSecurity::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host),
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            }
            SmtpSecurity::Plaintext => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                &config.host,
            )),
            _ => {
                return Err(MailError::Configuration {
                    host: config.host.clone(),
                    message: UNSUPPORTED_SECURITY.to_string(),
                });
            }
        }
        .map_err(|error| MailError::configuration(&config.host, &error))?
        .port(config.port)
        .timeout(Some(SEND_TIMEOUT));

        let builder = if config.authenticate {
            builder.credentials(Credentials::new(
                config.user.clone(),
                config.password.expose().to_string(),
            ))
        } else {
            builder
        };

        Ok(Self {
            transport: builder.build(),
            from,
            host: config.host.clone(),
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    fn compose(&self, recipient: &str, letter: &Letter) -> Result<Message, MailError> {
        let to: Address = recipient.parse().map_err(|_| MailError::Recipient)?;
        Message::builder()
            .from(self.from.clone())
            .to(Mailbox::new(None, to))
            .subject(letter.subject())
            .multipart(
                MultiPart::alternative()
                    .singlepart(part(ContentType::TEXT_PLAIN, letter.text()))
                    .singlepart(part(ContentType::TEXT_HTML, letter.html())),
            )
            .map_err(|error| MailError::Compose(error.to_string()))
    }
}

#[async_trait]
impl Outbox for Relay {
    async fn send(&self, recipient: &str, letter: &Letter) -> Result<(), MailError> {
        let message = self.compose(recipient, letter)?;
        match tokio::time::timeout(SEND_TIMEOUT, self.transport.send(message)).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(MailError::delivery(&self.host, &error)),
            Err(_) => Err(MailError::Timeout {
                host: self.host.clone(),
            }),
        }
    }
}

impl fmt::Debug for Relay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Relay")
            .field("host", &self.host)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

fn part(content_type: ContentType, body: &str) -> SinglePart {
    SinglePart::builder()
        .header(content_type)
        .header(ContentTransferEncoding::Base64)
        .body(body.to_string())
}

fn mailbox(sender: &Sender) -> Result<Mailbox, MailError> {
    let name = sender.name.clone().filter(|name| !name.trim().is_empty());
    if name
        .as_deref()
        .is_some_and(|name| name.contains(['\r', '\n']))
    {
        return Err(MailError::Sender);
    }
    let address: Address = sender.address.parse().map_err(|_| MailError::Sender)?;
    Ok(Mailbox::new(name, address))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    const PASSWORD: &str = "hunter2-not-a-real-password";

    fn config() -> SmtpConfig {
        SmtpConfig::new(
            "smtp.example.test",
            587,
            "postmaster",
            PASSWORD,
            Sender::new("licenses@example.test").with_name("Zone"),
        )
    }

    fn letter() -> Letter {
        Letter::license(
            "Acme Ltd",
            5,
            Utc.with_ymd_and_hms(2027, 11, 18, 0, 0, 0).unwrap(),
            "payload.signature",
        )
    }

    fn rendered(relay: &Relay) -> String {
        let message = relay
            .compose("buyer@example.test", &letter())
            .expect("composed");
        String::from_utf8(message.formatted()).expect("utf-8")
    }

    #[test]
    fn every_security_builds_with_and_without_credentials() {
        for security in [
            SmtpSecurity::Implicit,
            SmtpSecurity::StartTls,
            SmtpSecurity::Plaintext,
        ] {
            Relay::new(&config().with_security(security)).expect("signs in");
            Relay::new(&config().with_security(security).without_credentials()).expect("anonymous");
        }
    }

    #[test]
    fn debug_never_prints_the_credentials() {
        let rendered = format!("{:?}", Relay::new(&config()).expect("relay"));
        assert!(!rendered.contains(PASSWORD), "{rendered}");
        assert!(!rendered.contains("postmaster"), "{rendered}");
        assert!(rendered.contains("smtp.example.test"), "{rendered}");
    }

    #[test]
    fn the_message_is_multipart_alternative_from_the_configured_sender() {
        let message = rendered(&Relay::new(&config()).expect("relay"));
        assert!(
            message.contains("From: Zone <licenses@example.test>\r\n"),
            "{message}"
        );
        assert!(message.contains("To: buyer@example.test\r\n"), "{message}");
        assert!(
            message.contains("Subject: Your Zone Enterprise license key\r\n"),
            "{message}"
        );
        assert!(message.contains("multipart/alternative"), "{message}");
        assert!(
            message.contains("Content-Type: text/plain; charset=utf-8\r\n"),
            "{message}"
        );
        assert!(
            message.contains("Content-Type: text/html; charset=utf-8\r\n"),
            "{message}"
        );
        assert_eq!(
            message
                .matches("Content-Transfer-Encoding: base64\r\n")
                .count(),
            2,
            "{message}"
        );
    }

    #[test]
    fn a_blank_sender_name_sends_from_the_bare_address() {
        let mut config = config();
        config.sender = Sender::new("licenses@example.test").with_name("  ");
        let message = rendered(&Relay::new(&config).expect("relay"));
        assert!(
            message.contains("From: licenses@example.test\r\n"),
            "{message}"
        );
    }

    #[test]
    fn a_sender_name_with_a_line_break_is_refused() {
        let mut config = config();
        config.sender =
            Sender::new("licenses@example.test").with_name("Zone\r\nBcc: hidden@example.test");
        assert!(matches!(Relay::new(&config), Err(MailError::Sender)));
    }

    #[test]
    fn an_unparseable_sender_is_refused() {
        let mut config = config();
        config.sender = Sender::new("@@@");
        assert!(matches!(Relay::new(&config), Err(MailError::Sender)));
    }

    #[test]
    fn an_unparseable_recipient_is_refused_without_being_quoted() {
        let relay = Relay::new(&config()).expect("relay");
        let error = relay
            .compose("someone.private@@example.test", &letter())
            .expect_err("bad recipient");
        assert!(matches!(error, MailError::Recipient));
        assert!(!error.is_retryable());
        assert!(!error.to_string().contains("someone.private"), "{error}");
    }

    #[test]
    fn a_named_recipient_is_refused() {
        let relay = Relay::new(&config()).expect("relay");
        let error = relay
            .compose(
                "Sam\r\nBcc: hidden@example.test <buyer@example.test>",
                &letter(),
            )
            .expect_err("named recipient");
        assert!(matches!(error, MailError::Recipient));
    }
}

//! Sending account messages.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use abnegate_notify::Mail;
use abnegate_notify::Mailer;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::services::mail::Config;
use crate::services::mail::Error;
use crate::services::mail::Template;

/// How many dispatched messages may be with the relay at once. The endpoints
/// that dispatch are public, so without a bound each request could open an
/// SMTP session of its own.
pub const MAX_CONCURRENT_SENDS: usize = 8;

/// How long one dispatched message may take. lettre's async transport bounds
/// only the TCP connect: a relay that stalls after it would otherwise hold the
/// message's permit forever, and [`MAX_CONCURRENT_SENDS`] stalls would switch
/// account mail off until the server restarts.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(60);

/// Sends the account flow's messages through one [`Mail`].
#[derive(Clone)]
pub struct AccountMail {
    mailer: Arc<dyn Mail>,
    sends: Arc<Semaphore>,
}

impl AccountMail {
    pub fn new(mailer: Arc<dyn Mail>) -> Self {
        Self {
            mailer,
            sends: Arc::new(Semaphore::new(MAX_CONCURRENT_SENDS)),
        }
    }

    /// Mail through the relay `config` describes. Nothing connects until a
    /// message is sent.
    pub fn from_config(config: &Config) -> Result<Self, Error> {
        Ok(Self::new(Arc::new(Mailer::new(config.relay())?)))
    }

    pub async fn send(&self, recipient: &str, template: &Template<'_>) -> Result<(), Error> {
        self.mailer
            .send(recipient, template.subject(), &template.body())
            .await?;
        Ok(())
    }

    /// Send on a task of its own and log how it went, so a caller can answer
    /// without waiting on the relay.
    ///
    /// When [`MAX_CONCURRENT_SENDS`] messages are already with the relay, the
    /// message is dropped with a warning and `None` comes back. A send is given
    /// up after [`SEND_TIMEOUT`], which frees its place. Each account
    /// message carries a token that is stored before it is sent, so the person
    /// loses nothing by asking again.
    pub fn dispatch(&self, recipient: &str, template: &Template<'_>) -> Option<JoinHandle<()>> {
        let subject = template.subject();
        let Ok(permit) = Arc::clone(&self.sends).try_acquire_owned() else {
            tracing::warn!(subject, "Account mail is at capacity; dropping the message");
            return None;
        };
        let mailer = Arc::clone(&self.mailer);
        let recipient = recipient.to_string();
        let body = template.body();
        Some(tokio::spawn(
            async move {
                let _permit = permit;
                match tokio::time::timeout(SEND_TIMEOUT, mailer.send(&recipient, subject, &body))
                    .await
                {
                    Ok(Ok(())) => tracing::info!(subject, "Account mail sent"),
                    Ok(Err(error)) => {
                        tracing::error!(%error, subject, "Account mail did not go out")
                    }
                    Err(_) => tracing::error!(
                        subject,
                        "Account mail did not go out: the relay did not answer in time"
                    ),
                }
            }
            .in_current_span(),
        ))
    }
}

impl fmt::Debug for AccountMail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccountMail")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use abnegate_notify::MockMailer;
    use abnegate_notify::SentMail;
    use async_trait::async_trait;
    use tokio::io::AsyncBufReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::io::BufReader;
    use tokio::net::TcpListener;
    use tokio::net::TcpStream;
    use tokio::net::tcp::OwnedReadHalf;

    use super::*;
    use crate::services::mail::SenderPolicy;
    use crate::services::mail::Variable;

    const RECIPIENT: &str = "person@zone.test";
    const LINK: &str = "https://zone.test/reset-password?token=abc";
    const DEADLINE: Duration = Duration::from_secs(10);

    fn recorded() -> (AccountMail, MockMailer) {
        let mock = MockMailer::new();
        (AccountMail::new(Arc::new(mock.clone())), mock)
    }

    #[tokio::test]
    async fn every_message_reaches_the_mailer_with_its_subject_and_body() {
        let (mail, mock) = recorded();
        let verification = Template::Verification {
            name: "Sam",
            link: LINK,
        };
        let reset = Template::PasswordReset {
            name: "Sam",
            link: LINK,
        };

        mail.send(RECIPIENT, &verification).await.expect("sent");
        mail.send(RECIPIENT, &reset).await.expect("sent");

        assert_eq!(
            mock.sent().await,
            vec![
                SentMail::new(RECIPIENT, verification.subject(), verification.body()),
                SentMail::new(RECIPIENT, reset.subject(), reset.body()),
            ]
        );
    }

    #[tokio::test]
    async fn a_dispatched_message_reaches_the_mailer_on_its_own_task() {
        let (mail, mock) = recorded();
        let reset = Template::PasswordReset {
            name: "Sam",
            link: LINK,
        };

        let sending = mail.dispatch(RECIPIENT, &reset).expect("a free permit");
        tokio::time::timeout(DEADLINE, sending)
            .await
            .expect("the send finishes")
            .expect("the send did not panic");
        assert_eq!(
            mail.sends.available_permits(),
            MAX_CONCURRENT_SENDS,
            "a finished send gives its permit back"
        );

        assert_eq!(
            mock.sent().await,
            vec![SentMail::new(RECIPIENT, reset.subject(), reset.body())]
        );
    }

    /// A relay that never answers, so every send it is given stays in flight.
    /// It counts the sends that have reached it.
    #[derive(Clone, Default)]
    struct Silent {
        entered: Arc<AtomicUsize>,
    }

    impl Silent {
        /// Yield until `count` sends are inside [`Mail::send`].
        async fn reached(&self, count: usize) {
            tokio::time::timeout(DEADLINE, async {
                while self.entered.load(Ordering::SeqCst) < count {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("every dispatched send reaches the relay");
        }
    }

    #[async_trait]
    impl Mail for Silent {
        async fn send(
            &self,
            _recipient: &str,
            _subject: &str,
            _body: &str,
        ) -> Result<(), abnegate_notify::Error> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        }
    }

    fn reset() -> Template<'static> {
        Template::PasswordReset {
            name: "Sam",
            link: LINK,
        }
    }

    #[tokio::test]
    async fn a_message_past_the_concurrency_bound_is_dropped_without_a_task() {
        let silent = Silent::default();
        let mail = AccountMail::new(Arc::new(silent.clone()));

        let in_flight: Vec<JoinHandle<()>> = (0..MAX_CONCURRENT_SENDS)
            .map(|_| mail.dispatch(RECIPIENT, &reset()).expect("a free permit"))
            .collect();
        silent.reached(MAX_CONCURRENT_SENDS).await;

        assert!(
            mail.dispatch(RECIPIENT, &reset()).is_none(),
            "every permit is held by a send that is with the relay"
        );
        assert_eq!(mail.sends.available_permits(), 0);
        assert_eq!(
            silent.entered.load(Ordering::SeqCst),
            MAX_CONCURRENT_SENDS,
            "the dropped message never reached the relay"
        );

        for sending in in_flight {
            sending.abort();
            assert!(sending.await.expect_err("aborted").is_cancelled());
        }
        assert_eq!(
            mail.sends.available_permits(),
            MAX_CONCURRENT_SENDS,
            "an ended send gives its permit back"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_relay_that_never_answers_gives_its_permit_back_after_the_send_timeout() {
        let silent = Silent::default();
        let mail = AccountMail::new(Arc::new(silent.clone()));

        let sending = mail.dispatch(RECIPIENT, &reset()).expect("a free permit");
        silent.reached(1).await;
        assert_eq!(mail.sends.available_permits(), MAX_CONCURRENT_SENDS - 1);

        tokio::time::advance(SEND_TIMEOUT + Duration::from_secs(1)).await;
        tokio::time::timeout(DEADLINE, sending)
            .await
            .expect("the send is given up once SEND_TIMEOUT passes")
            .expect("the send did not panic");

        assert_eq!(mail.sends.available_permits(), MAX_CONCURRENT_SENDS);
    }

    /// One line from the client, or what arrived before it hung up.
    async fn command(reader: &mut BufReader<OwnedReadHalf>) -> String {
        let mut line = Vec::new();
        let read = tokio::time::timeout(DEADLINE, reader.read_until(b'\n', &mut line)).await;
        if !matches!(read, Ok(Ok(_))) {
            line.clear();
        }
        String::from_utf8_lossy(&line).trim_end().to_string()
    }

    /// A relay that greets in plaintext, offers STARTTLS, and records the
    /// client's first two commands before hanging up.
    async fn plaintext_relay(stream: TcpStream) -> Vec<String> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut commands = Vec::new();

        if writer
            .write_all(b"220 relay.zone.test ESMTP\r\n")
            .await
            .is_err()
        {
            return commands;
        }
        commands.push(command(&mut reader).await);

        if writer
            .write_all(b"250-relay.zone.test\r\n250-STARTTLS\r\n250 AUTH PLAIN LOGIN\r\n")
            .await
            .is_err()
        {
            return commands;
        }
        commands.push(command(&mut reader).await);
        commands
    }

    /// `SMTP_PORT` defaults to 587, the submission port relays serve with
    /// STARTTLS. Every port but 465 must greet in plaintext and upgrade before
    /// the credentials go out; an implicit-TLS client opens with a ClientHello
    /// instead and never gets past the relay's greeting.
    #[tokio::test]
    async fn a_submission_port_relay_is_upgraded_with_starttls_before_signing_in() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local port");
        let port = listener.local_addr().expect("bound").port();
        let relay = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("the mailer connects");
            plaintext_relay(stream).await
        });

        let config = Config::from_variables(
            |variable| match variable {
                Variable::Host => Some("127.0.0.1".to_string()),
                Variable::Port => Some(port.to_string()),
                Variable::User => Some("zone".to_string()),
                Variable::Password => Some("hunter2-not-a-real-password".to_string()),
                _ => None,
            },
            SenderPolicy::Default,
        )
        .expect("configured");
        let mail = AccountMail::from_config(&config).expect("a mailer");

        let outcome = tokio::time::timeout(
            DEADLINE,
            mail.send(
                RECIPIENT,
                &Template::PasswordReset {
                    name: "Sam",
                    link: LINK,
                },
            ),
        )
        .await
        .expect("the send finishes once the relay hangs up");
        let commands = tokio::time::timeout(DEADLINE, relay)
            .await
            .expect("the relay finishes")
            .expect("the relay did not panic");

        assert!(
            commands
                .first()
                .is_some_and(|line| line.starts_with("EHLO ")),
            "the client must answer the plaintext greeting with EHLO, got {commands:?}"
        );
        assert_eq!(
            commands.get(1).map(String::as_str),
            Some("STARTTLS"),
            "the client must upgrade before anything else, got {commands:?}"
        );
        assert!(
            outcome.is_err(),
            "a relay that hangs up mid-upgrade cannot have taken the message"
        );
    }
}

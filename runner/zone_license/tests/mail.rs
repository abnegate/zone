mod support;

use abnegate_notify::{Sender, SmtpConfig, SmtpSecurity};
use chrono::{TimeZone, Utc};
use tokio::net::TcpListener;
use zone_enterprise::Plan;
use zone_license::mail::{Failure, Letter, MailError, Outbox, Relay};

use crate::support::smtp::{SENDER, SmtpSink};

const RECIPIENT: &str = "billing@acme.test";
const CUSTOMER: &str = "Acme <Ltd> & Sons";
const KEY: &str = concat!(
    "eyJpZCI6IjAxOTM2YjJlLTRmNWEtNWI2Yy04ZDdlLTlmMGExYjJjM2Q0ZSIsImN1c3RvbWVyIjoiQWNtZSBMdGQiLCJz",
    "ZWF0cyI6NSwiaXNzdWVkX2F0IjoiMjAyNi0xMS0xOFQxMTo1OTo1OFoiLCJleHBpcmVzX2F0IjoiMjAyNy0xMS0xOFQx",
    "MTo1OTo1OFoiLCJrZXlfaWQiOiIyMDI2LTEifQ.",
    "3q2-7wABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4fICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj9A",
);

fn letter() -> Letter {
    Letter::license(
        CUSTOMER,
        Plan::Enterprise,
        5,
        Utc.with_ymd_and_hms(2027, 11, 18, 11, 59, 58).unwrap(),
        KEY,
    )
}

async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    listener.local_addr().expect("address").port()
}

#[tokio::test]
async fn a_license_reaches_the_relay_as_text_and_html_with_the_key_intact() {
    let sink = SmtpSink::start().await;
    let relay = Relay::new(&sink.config()).expect("relay");

    relay.send(RECIPIENT, &letter()).await.expect("sent");

    let messages = sink.messages();
    assert_eq!(messages.len(), 1);
    let message = &messages[0];
    assert_eq!(message.recipients, vec![RECIPIENT.to_string()]);
    assert_eq!(
        message.header("Subject"),
        Some("Your Zone Enterprise license key")
    );
    assert_eq!(
        message.header("From"),
        Some(format!("Zone <{SENDER}>").as_str())
    );
    assert!(
        message
            .header("Content-Type")
            .is_some_and(|value| value.starts_with("multipart/alternative")),
        "{}",
        message.data
    );

    let parts = message.parts();
    assert_eq!(parts.len(), 2, "{parts:?}");

    let text = message.part("text/plain").expect("text part");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.contains(&KEY), "{text}");
    assert!(lines.contains(&"Customer: Acme <Ltd> & Sons"), "{text}");
    assert!(lines.contains(&"Paid through: 18 November 2027"), "{text}");

    let html = message.part("text/html").expect("html part");
    assert!(html.contains(&format!(">{KEY}</pre>")), "{html}");
    assert!(
        html.contains("<td>Acme &lt;Ltd&gt; &amp; Sons</td>"),
        "{html}"
    );
}

#[tokio::test]
async fn a_team_license_reaches_the_relay_under_a_team_subject() {
    let sink = SmtpSink::start().await;
    let relay = Relay::new(&sink.config()).expect("relay");
    let letter = Letter::license(
        CUSTOMER,
        Plan::Team,
        5,
        Utc.with_ymd_and_hms(2027, 11, 18, 11, 59, 58).unwrap(),
        KEY,
    );

    relay.send(RECIPIENT, &letter).await.expect("sent");

    let messages = sink.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].header("Subject"),
        Some("Your Zone Team license key")
    );
    let text = messages[0].part("text/plain").expect("text part");
    assert!(text.lines().any(|line| line == "Plan: Team"), "{text}");
}

#[tokio::test]
async fn a_refused_connection_is_retryable() {
    let config = SmtpConfig::unauthenticated("127.0.0.1", closed_port().await, Sender::new(SENDER))
        .with_security(SmtpSecurity::Plaintext);
    let relay = Relay::new(&config).expect("relay");

    let error = relay.send(RECIPIENT, &letter()).await.expect_err("refused");

    assert!(matches!(error, MailError::Delivery { .. }), "{error:?}");
    assert!(error.is_retryable(), "{error}");
}

#[tokio::test]
async fn a_transient_rejection_is_retryable() {
    let sink = SmtpSink::rejecting("451 4.3.0 try again later").await;
    let relay = Relay::new(&sink.config()).expect("relay");

    let error = relay
        .send(RECIPIENT, &letter())
        .await
        .expect_err("rejected");

    assert!(error.is_retryable(), "{error}");
    assert!(sink.messages().is_empty());
}

#[tokio::test]
async fn a_permanent_rejection_is_not_retryable() {
    let sink = SmtpSink::rejecting("550 5.1.1 no such user").await;
    let relay = Relay::new(&sink.config()).expect("relay");

    let error = relay
        .send(RECIPIENT, &letter())
        .await
        .expect_err("rejected");

    assert!(matches!(error, MailError::Delivery { .. }), "{error:?}");
    assert!(!error.is_retryable(), "{error}");
    assert!(sink.messages().is_empty());
}

#[tokio::test]
async fn a_rejection_that_quotes_the_recipient_keeps_only_its_status() {
    let sink =
        SmtpSink::rejecting("550 5.1.1 <billing@acme.test>: Recipient address rejected").await;
    let relay = Relay::new(&sink.config()).expect("relay");

    let error = relay
        .send(RECIPIENT, &letter())
        .await
        .expect_err("rejected");

    for shown in [
        error.to_string(),
        format!("{error:?}"),
        format!("{error:#?}"),
    ] {
        assert!(!shown.contains("billing@acme.test"), "{shown}");
        assert!(!shown.contains("Recipient address rejected"), "{shown}");
    }
    assert!(
        matches!(
            error,
            MailError::Delivery {
                failure: Failure::Rejected,
                status: Some(550),
                permanent: true,
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_unparseable_recipient_never_reaches_the_relay() {
    let sink = SmtpSink::start().await;
    let relay = Relay::new(&sink.config()).expect("relay");

    let error = relay
        .send("not an address", &letter())
        .await
        .expect_err("refused");

    assert!(matches!(error, MailError::Recipient));
    assert!(!error.is_retryable());
    assert!(sink.messages().is_empty());
}

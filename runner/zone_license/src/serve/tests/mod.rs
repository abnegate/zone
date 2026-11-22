use std::collections::HashMap;

use zone_enterprise::{Keyring, Plan, TrustedKey};

use super::*;
use crate::logging;
use crate::private_key::PrivateKey;
use crate::variable::Variable;

const SECRET: &str = "whsec_serve-test-secret";
const KEY_ID: &str = "test-serve-1";
const SEED: [u8; 32] = [7; 32];
const ENTERPRISE: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
const TEAM: &str = "8a2b6c1d-4e3f-4a5b-9c6d-7e8f9a0b1c2d";

fn config(products: &str) -> Config {
    let key = PrivateKey::new(SEED);
    let keyring = Keyring::new([TrustedKey::new(KEY_ID, key.public()).unwrap()]).unwrap();
    let variables = HashMap::from([
        (Variable::WebhookSecret, SECRET.to_owned()),
        (Variable::Products, products.to_owned()),
        (Variable::PrivateKey, key.encode()),
        (Variable::KeyId, KEY_ID.to_owned()),
        (Variable::Listen, "127.0.0.1:0".to_owned()),
        (Variable::SmtpHost, "127.0.0.1".to_owned()),
        (Variable::SmtpUser, "unused".to_owned()),
        (Variable::SmtpPassword, String::new()),
        (Variable::SmtpFrom, "licenses@zone.test".to_owned()),
    ]);
    Config::from_variables(|variable| variables.get(&variable).cloned(), &keyring).unwrap()
}

#[tokio::test]
async fn start_logs_each_product_with_the_plan_it_issues() {
    let config = config(&format!("{ENTERPRISE},{TEAM}=team"));

    let logged = logging::capture(tracing::Level::INFO, async {
        start(config).await.unwrap();
    })
    .await;

    for (product, plan) in [(ENTERPRISE, Plan::Enterprise), (TEAM, Plan::Team)] {
        let line = logged
            .lines()
            .find(|line| line.contains(&format!("product=\"{product}\"")))
            .unwrap_or_else(|| panic!("{product} missing from {logged}"));
        assert!(
            line.contains(&format!("plan={plan}")),
            "{product} logged without {plan}: {line}"
        );
    }
}

#[tokio::test]
async fn start_never_logs_the_webhook_secret_or_the_private_key() {
    let config = config(ENTERPRISE);
    let key = PrivateKey::new(SEED).encode();

    let logged = logging::capture(tracing::Level::TRACE, async {
        start(config).await.unwrap();
    })
    .await;

    assert!(!logged.is_empty());
    for secret in [SECRET, key.as_str()] {
        assert!(!logged.contains(secret), "{secret} logged in {logged}");
    }
}

fn bundled(proxies: Option<&str>) -> Config {
    let key = PrivateKey::new(SEED);
    let keyring = Keyring::new([TrustedKey::new(KEY_ID, key.public()).unwrap()]).unwrap();
    let mut variables = HashMap::from([
        (Variable::WebhookSecret, SECRET.to_owned()),
        (Variable::Products, ENTERPRISE.to_owned()),
        (Variable::PrivateKey, key.encode()),
        (Variable::KeyId, KEY_ID.to_owned()),
        (Variable::Listen, "127.0.0.1:0".to_owned()),
        (Variable::SmtpHost, "127.0.0.1".to_owned()),
        (Variable::SmtpUser, "unused".to_owned()),
        (Variable::SmtpPassword, String::new()),
        (Variable::SmtpFrom, "licenses@zone.test".to_owned()),
        (Variable::BundleBucket, BUNDLE_BUCKET.to_owned()),
        (Variable::BundleRegion, "ap-southeast-2".to_owned()),
        (Variable::BundlePrefix, "zone/bundles".to_owned()),
        (Variable::BundleAccessKeyId, BUNDLE_ACCESS_KEY.to_owned()),
        (Variable::BundleSecretAccessKey, BUNDLE_SECRET.to_owned()),
    ]);
    if let Some(proxies) = proxies {
        variables.insert(Variable::TrustedProxies, proxies.to_owned());
    }
    Config::from_variables(|variable| variables.get(&variable).cloned(), &keyring).unwrap()
}

const BUNDLE_BUCKET: &str = "zone-distribution";
const BUNDLE_ACCESS_KEY: &str = "AKIAZONESERVESTART01";
const BUNDLE_SECRET: &str = "serve-start-bundle-secret";

#[tokio::test]
async fn start_logs_the_bundle_bucket_and_prefix_but_no_credential() {
    let logged = logging::capture(tracing::Level::TRACE, async {
        start(bundled(Some("172.18.0.0/16"))).await.unwrap();
    })
    .await;

    let line = logged
        .lines()
        .find(|line| line.contains("Serving air-gapped bundles"))
        .unwrap_or_else(|| panic!("no bundle line in {logged}"));
    assert!(
        line.contains(&format!("bucket=\"{BUNDLE_BUCKET}\"")),
        "{line}"
    );
    assert!(line.contains("prefix=\"zone/bundles\""), "{line}");
    for secret in [BUNDLE_ACCESS_KEY, BUNDLE_SECRET] {
        assert!(!logged.contains(secret), "{secret} logged in {logged}");
    }
    assert!(logged.contains("proxies=172.18.0.0/16"), "{logged}");
    assert!(logged.contains("Listening for Polar webhooks"), "{logged}");
}

#[tokio::test]
async fn start_warns_once_when_bundles_are_on_without_trusted_proxies() {
    let logged = logging::capture(tracing::Level::INFO, async {
        start(bundled(None)).await.unwrap();
    })
    .await;

    let warnings = logged
        .lines()
        .filter(|line| line.contains("No trusted proxies are set"))
        .count();
    assert_eq!(warnings, 1, "{logged}");
}

#[tokio::test]
async fn start_announces_nothing_about_proxies_while_bundles_are_off() {
    let logged = logging::capture(tracing::Level::INFO, async {
        start(config(ENTERPRISE)).await.unwrap();
    })
    .await;

    assert!(!logged.contains("trusted proxy"), "{logged}");
    assert!(!logged.contains("No trusted proxies"), "{logged}");
    assert!(!logged.contains("Serving air-gapped bundles"), "{logged}");
}

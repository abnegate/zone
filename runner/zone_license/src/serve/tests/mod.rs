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

use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::num::NonZeroU32;
use std::path::Path;

use abnegate_notify::{Sender, SmtpConfig};
use lettre::Address;
use rusty_s3::{Bucket, Credentials, UrlStyle};
use zone_enterprise::Keyring;

use crate::bundle::{BundleSettings, DEFAULT_PREFIX};
use crate::config_error::ConfigError;
use crate::gate::{DEFAULT_ADDRESS_LIMIT, DEFAULT_KEY_LIMIT, Limits, Revoked, TrustedProxies};
use crate::polar::{AccessToken, WebhookSecret};
use crate::private_key::PrivateKey;
use crate::problem::Problem;
use crate::products::Products;
use crate::registry::{RegistrySettings, Repositories};
use crate::signer::Signer;
use crate::variable::Variable;

pub const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080));
pub const DEFAULT_SMTP_PORT: u16 = 587;
pub const DEFAULT_SENDER_NAME: &str = "Zone";
pub const DEFAULT_REGISTRY_KEY_GENERATION: NonZeroU32 = NonZeroU32::MIN;

const LISTEN_REASON: &str = "it must be an IP address and port, such as 0.0.0.0:8080";
const PORT_REASON: &str = "it must be a port number from 1 to 65535";
const FROM_REASON: &str = "it must be a bare email address, such as licenses@example.com";
const FROM_NAME_REASON: &str = "it must be a single line";
const LIMIT_REASON: &str = "it must be a whole number of requests a minute, at least 1";
const PROXIES_REASON: &str = "registry tokens and bundles need it. Set it to the reverse proxy's network, such as 172.18.0.0/16, or to none when no proxy is in front of the signer";
const SERVICE_REASON: &str =
    "it must be the registry's name with no spaces, such as registry.bitecode.ltd";
const GENERATION_REASON: &str = "it must be a whole number from 1";
const REGION_REASON: &str =
    "it must be a region name of lowercase letters, digits and dashes, such as ap-southeast-2";
const ENDPOINT_REASON: &str =
    "it must be an http or https URL with a host, such as https://s3.ap-southeast-2.amazonaws.com";
const PREFIX_REASON: &str =
    "it must be one or more path segments of letters, digits, '.', '_' or '-', such as bundles";

pub struct Config {
    pub signer: Signer,
    pub secret: WebhookSecret,
    pub products: Products,
    pub access_token: Option<AccessToken>,
    pub keyring: Keyring,
    pub listen: SocketAddr,
    pub smtp: SmtpConfig,
    pub revoked: Revoked,
    pub proxies: TrustedProxies,
    pub limits: Limits,
    pub registry: Option<RegistrySettings>,
    pub bundles: Option<BundleSettings>,
}

impl Config {
    pub fn from_environment(keyring: &Keyring) -> Result<Self, ConfigError> {
        Self::from_variables(Variable::exact, keyring)
    }

    /// Reads every variable through `read`, which returns values exactly as
    /// set. A value that is blank once trimmed counts as unset, except
    /// `SMTP_PASSWORD`, which is kept as given because a relay may accept an
    /// empty one. Every problem is reported, not just the first.
    pub fn from_variables(
        read: impl Fn(Variable) -> Option<String>,
        keyring: &Keyring,
    ) -> Result<Self, ConfigError> {
        let value = |variable: Variable| {
            read(variable)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let mut problems = Vec::new();

        let secret = required(
            value(Variable::WebhookSecret),
            Variable::WebhookSecret,
            &mut problems,
        );
        let products = required(value(Variable::Products), Variable::Products, &mut problems)
            .and_then(|list| {
                Products::parse(&list)
                    .map_err(|error| {
                        problems.push(invalid(Variable::Products, &error.to_string()));
                    })
                    .ok()
            });
        let access_token = value(Variable::AccessToken).map(AccessToken::new);
        let listen = parsed(
            value(Variable::Listen),
            Variable::Listen,
            DEFAULT_LISTEN,
            LISTEN_REASON,
            &mut problems,
            |text| text.parse().ok(),
        );
        let signer = signer(&value, keyring, &mut problems);

        let host = required(value(Variable::SmtpHost), Variable::SmtpHost, &mut problems);
        let port = parsed(
            value(Variable::SmtpPort),
            Variable::SmtpPort,
            DEFAULT_SMTP_PORT,
            PORT_REASON,
            &mut problems,
            |text| text.parse().ok().filter(|port| *port != 0),
        );
        let user = required(value(Variable::SmtpUser), Variable::SmtpUser, &mut problems);
        let password = required(
            read(Variable::SmtpPassword),
            Variable::SmtpPassword,
            &mut problems,
        );
        let from = required(value(Variable::SmtpFrom), Variable::SmtpFrom, &mut problems).and_then(
            |from| {
                checked(
                    from,
                    Variable::SmtpFrom,
                    FROM_REASON,
                    &mut problems,
                    |from| from.parse::<Address>().is_ok(),
                )
            },
        );
        let name = checked(
            value(Variable::SmtpFromName).unwrap_or_else(|| DEFAULT_SENDER_NAME.to_owned()),
            Variable::SmtpFromName,
            FROM_NAME_REASON,
            &mut problems,
            |name| !name.contains(['\r', '\n']),
        );

        let revoked_licenses = listed(
            value(Variable::Revoked),
            Variable::Revoked,
            &mut problems,
            Revoked::parse_licenses,
        );
        let revoked_subscriptions = listed(
            value(Variable::RevokedSubscriptions),
            Variable::RevokedSubscriptions,
            &mut problems,
            Revoked::parse_subscriptions,
        );
        let proxies = listed(
            value(Variable::TrustedProxies),
            Variable::TrustedProxies,
            &mut problems,
            TrustedProxies::parse,
        );
        let address_limit = limit(
            value(Variable::AddressLimit),
            Variable::AddressLimit,
            DEFAULT_ADDRESS_LIMIT,
            &mut problems,
        );
        let key_limit = limit(
            value(Variable::KeyLimit),
            Variable::KeyLimit,
            DEFAULT_KEY_LIMIT,
            &mut problems,
        );
        let registry = registry(&value, &mut problems);
        let bundles = bundles(&value, &mut problems);
        let gated =
            value(Variable::RegistryService).is_some() || value(Variable::BundleBucket).is_some();
        if gated && value(Variable::TrustedProxies).is_none() {
            problems.push(Problem::Required {
                variable: Variable::TrustedProxies,
                reason: PROXIES_REASON,
            });
        }

        let assembled = (|| {
            Some(Self {
                signer: signer?,
                secret: WebhookSecret::new(&secret?),
                products: products?,
                access_token,
                keyring: keyring.clone(),
                listen: listen?,
                smtp: SmtpConfig::new(
                    host?,
                    port?,
                    user?,
                    password?,
                    Sender::new(from?).with_name(name?),
                ),
                revoked: Revoked::new(revoked_licenses?, revoked_subscriptions?),
                proxies: proxies?,
                limits: Limits {
                    address: address_limit?,
                    key: key_limit?,
                },
                registry: registry?,
                bundles,
            })
        })();
        match assembled {
            Some(config) if problems.is_empty() => Ok(config),
            _ => Err(ConfigError::new(problems)),
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("signer", &self.signer)
            .field("secret", &self.secret)
            .field("products", &self.products)
            .field("access_token", &self.access_token)
            .field("listen", &self.listen)
            .field("smtp_host", &self.smtp.host)
            .field("smtp_port", &self.smtp.port)
            .field("smtp_user", &self.smtp.user)
            .field("smtp_from", &self.smtp.sender.address)
            .field("smtp_from_name", &self.smtp.sender.name)
            .field("revoked", &self.revoked)
            .field("proxies", &self.proxies)
            .field("limits", &self.limits)
            .field("registry", &self.registry)
            .field("bundles", &self.bundles)
            .finish_non_exhaustive()
    }
}

fn signer(
    value: &impl Fn(Variable) -> Option<String>,
    keyring: &Keyring,
    problems: &mut Vec<Problem>,
) -> Option<Signer> {
    let file = value(Variable::PrivateKeyFile);
    let key = PrivateKey::resolve(
        file.as_deref().map(Path::new),
        value(Variable::PrivateKey).as_deref(),
    )
    .map_err(|error| problems.push(error.into()))
    .ok();
    let key_id = required(value(Variable::KeyId), Variable::KeyId, problems);
    let signer = Signer::new(key?, key_id?);
    signer
        .trusted_by(keyring)
        .map_err(|error| problems.push(error.into()))
        .ok()?;
    Some(signer)
}

fn registry(
    value: &impl Fn(Variable) -> Option<String>,
    problems: &mut Vec<Problem>,
) -> Option<Option<RegistrySettings>> {
    let service = value(Variable::RegistryService).map(|service| {
        checked(
            service,
            Variable::RegistryService,
            SERVICE_REASON,
            problems,
            |service| {
                !service
                    .chars()
                    .any(|character| character.is_whitespace() || character.is_control())
            },
        )
    });
    let repositories = match value(Variable::RegistryRepositories) {
        None => Some(Repositories::default()),
        Some(list) => Repositories::parse(&list)
            .map_err(|error| {
                problems.push(invalid(Variable::RegistryRepositories, &error.to_string()));
            })
            .ok(),
    };
    let generation = parsed(
        value(Variable::RegistryKeyGeneration),
        Variable::RegistryKeyGeneration,
        DEFAULT_REGISTRY_KEY_GENERATION,
        GENERATION_REASON,
        problems,
        |text| text.parse().ok(),
    );
    match service {
        None => Some(None),
        Some(service) => Some(Some(RegistrySettings {
            service: service?,
            repositories: repositories?,
            generation: generation?,
        })),
    }
}

fn bundles(
    value: &impl Fn(Variable) -> Option<String>,
    problems: &mut Vec<Problem>,
) -> Option<BundleSettings> {
    let bucket = value(Variable::BundleBucket)?;
    let region = required(
        value(Variable::BundleRegion),
        Variable::BundleRegion,
        problems,
    )
    .and_then(|region| {
        checked(
            region,
            Variable::BundleRegion,
            REGION_REASON,
            problems,
            BundleSettings::is_region,
        )
    });
    let endpoint = match value(Variable::BundleEndpoint) {
        Some(endpoint) => checked(
            endpoint,
            Variable::BundleEndpoint,
            ENDPOINT_REASON,
            problems,
            BundleSettings::is_endpoint,
        ),
        None => region.as_deref().map(BundleSettings::default_endpoint),
    };
    let prefix = parsed(
        value(Variable::BundlePrefix),
        Variable::BundlePrefix,
        DEFAULT_PREFIX.to_owned(),
        PREFIX_REASON,
        problems,
        BundleSettings::parse_prefix,
    );
    let key = required(
        value(Variable::BundleAccessKeyId),
        Variable::BundleAccessKeyId,
        problems,
    );
    let secret = required(
        value(Variable::BundleSecretAccessKey),
        Variable::BundleSecretAccessKey,
        problems,
    );
    let bucket = match (endpoint.and_then(|endpoint| endpoint.parse().ok()), region) {
        (Some(endpoint), Some(region)) => Bucket::new(endpoint, UrlStyle::Path, bucket, region)
            .map_err(|_| problems.push(invalid(Variable::BundleEndpoint, ENDPOINT_REASON)))
            .ok(),
        _ => None,
    };
    Some(BundleSettings {
        bucket: bucket?,
        prefix: prefix?,
        credentials: Credentials::new(key?, secret?),
    })
}

fn required(
    found: Option<String>,
    variable: Variable,
    problems: &mut Vec<Problem>,
) -> Option<String> {
    if found.is_none() {
        problems.push(Problem::Missing(variable));
    }
    found
}

fn parsed<T>(
    found: Option<String>,
    variable: Variable,
    default: T,
    reason: &str,
    problems: &mut Vec<Problem>,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Option<T> {
    let Some(text) = found else {
        return Some(default);
    };
    let parsed = parse(&text);
    if parsed.is_none() {
        problems.push(invalid(variable, reason));
    }
    parsed
}

fn listed<T: Default, E: fmt::Display>(
    found: Option<String>,
    variable: Variable,
    problems: &mut Vec<Problem>,
    parse: impl FnOnce(&str) -> Result<T, E>,
) -> Option<T> {
    let Some(text) = found else {
        return Some(T::default());
    };
    parse(&text)
        .map_err(|error| problems.push(invalid(variable, &error.to_string())))
        .ok()
}

fn limit(
    found: Option<String>,
    variable: Variable,
    default: NonZeroU32,
    problems: &mut Vec<Problem>,
) -> Option<NonZeroU32> {
    parsed(found, variable, default, LIMIT_REASON, problems, |text| {
        text.parse().ok()
    })
}

fn checked(
    found: String,
    variable: Variable,
    reason: &str,
    problems: &mut Vec<Problem>,
    valid: impl FnOnce(&str) -> bool,
) -> Option<String> {
    if valid(&found) {
        Some(found)
    } else {
        problems.push(invalid(variable, reason));
        None
    }
}

fn invalid(variable: Variable, reason: &str) -> Problem {
    Problem::Invalid {
        variable,
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::io::Write;

    use zone_enterprise::{Plan, TrustedKey};

    use super::*;
    use crate::gate::NONE;
    use crate::private_key_error::PrivateKeyError;
    use crate::untrusted::Untrusted;

    const SEED: [u8; 32] = [11; 32];
    const KEY_ID: &str = "test-1";
    const SECRET: &str = "whsec_config-test-secret";
    const PASSWORD: &str = "hunter2-not-a-real-password";
    const PRODUCT: &str = "prod_a";
    const ACCESS_TOKEN: &str = "polar_oat_config-test-token";
    const REVOKED_UPPER: &str = "0193A8E2-7C4B-7D10-8F00-4D2C5A9B1E01";
    const REVOKED_OTHER: &str = "0193a8e2-7c4b-7d10-8f00-4d2c5a9b1e02";
    const REVOKED_SUBSCRIPTION: &str = "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c";
    const BUCKET: &str = "zone-distribution";
    const REGION: &str = "ap-southeast-2";
    const ACCESS_KEY_ID: &str = "AKIACONFIGBUNDLETEST";
    const SECRET_ACCESS_KEY: &str = "config-bundle-secret/hidden+key";
    const REQUIRED: [Variable; 7] = [
        Variable::WebhookSecret,
        Variable::Products,
        Variable::KeyId,
        Variable::SmtpHost,
        Variable::SmtpUser,
        Variable::SmtpPassword,
        Variable::SmtpFrom,
    ];

    fn keyring() -> Keyring {
        Keyring::new([TrustedKey::new(KEY_ID, PrivateKey::new(SEED).public()).unwrap()]).unwrap()
    }

    fn complete() -> HashMap<Variable, String> {
        HashMap::from([
            (Variable::WebhookSecret, SECRET.to_owned()),
            (Variable::Products, PRODUCT.to_owned()),
            (Variable::PrivateKey, PrivateKey::new(SEED).encode()),
            (Variable::KeyId, KEY_ID.to_owned()),
            (Variable::SmtpHost, "smtp.example.test".to_owned()),
            (Variable::SmtpUser, "postmaster".to_owned()),
            (Variable::SmtpPassword, PASSWORD.to_owned()),
            (Variable::SmtpFrom, "licenses@example.test".to_owned()),
        ])
    }

    fn load(variables: &HashMap<Variable, String>) -> Result<Config, ConfigError> {
        Config::from_variables(|variable| variables.get(&variable).cloned(), &keyring())
    }

    fn with(changes: &[(Variable, &str)]) -> HashMap<Variable, String> {
        let mut variables = complete();
        for (variable, value) in changes {
            variables.insert(*variable, (*value).to_owned());
        }
        variables
    }

    fn without(removed: Variable) -> HashMap<Variable, String> {
        let mut variables = complete();
        variables.remove(&removed);
        variables
    }

    fn problems(variables: &HashMap<Variable, String>) -> Vec<Problem> {
        load(variables).expect_err("refused").into_problems()
    }

    fn invalid(problems: &[Problem]) -> Vec<Variable> {
        problems
            .iter()
            .filter_map(|problem| match problem {
                Problem::Invalid { variable, .. } => Some(*variable),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_required_variables_alone_configure_the_defaults() {
        let config = load(&complete()).unwrap();
        assert_eq!(config.signer.key_id(), KEY_ID);
        assert_eq!(config.signer.public(), PrivateKey::new(SEED).public());
        assert_eq!(config.products.plan(PRODUCT), Some(Plan::Enterprise));
        assert_eq!(config.products.len(), 1);
        assert_eq!(config.listen, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.smtp.host, "smtp.example.test");
        assert_eq!(config.smtp.port, DEFAULT_SMTP_PORT);
        assert_eq!(config.smtp.user, "postmaster");
        assert_eq!(config.smtp.password.expose(), PASSWORD);
        assert_eq!(
            config.smtp.sender,
            Sender::new("licenses@example.test").with_name(DEFAULT_SENDER_NAME)
        );
    }

    #[test]
    fn every_optional_variable_is_honoured() {
        let config = load(&with(&[
            (Variable::Listen, "127.0.0.1:9000"),
            (Variable::SmtpPort, "465"),
            (Variable::SmtpFromName, "Zone Licensing"),
        ]))
        .unwrap();
        assert_eq!(config.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.smtp.port, 465);
        assert_eq!(config.smtp.sender.name.as_deref(), Some("Zone Licensing"));
    }

    #[test]
    fn each_missing_required_variable_is_named() {
        for missing in REQUIRED {
            let problems = problems(&without(missing));
            assert!(
                matches!(problems.as_slice(), [Problem::Missing(variable)] if *variable == missing),
                "{missing:?}: {problems:?}"
            );
        }
    }

    #[test]
    fn a_blank_required_variable_counts_as_missing() {
        for blank in REQUIRED
            .into_iter()
            .filter(|variable| *variable != Variable::SmtpPassword)
        {
            let problems = problems(&with(&[(blank, "  ")]));
            assert!(
                matches!(problems.as_slice(), [Problem::Missing(variable)] if *variable == blank),
                "{blank:?}: {problems:?}"
            );
        }
    }

    #[test]
    fn a_blank_password_is_kept_for_a_relay_that_takes_an_empty_one() {
        for password in ["", "  "] {
            let config = load(&with(&[(Variable::SmtpPassword, password)])).unwrap();
            assert_eq!(config.smtp.password.expose(), password);
        }
    }

    #[test]
    fn an_empty_environment_names_every_required_variable_at_once() {
        let error = load(&HashMap::new()).expect_err("nothing set");
        let message = error.to_string();
        for variable in REQUIRED {
            assert!(message.contains(variable.name()), "{message}");
        }
        assert!(
            message.contains(Variable::PrivateKeyFile.name()),
            "{message}"
        );
        assert!(message.contains(Variable::PrivateKey.name()), "{message}");
        assert_eq!(error.problems().len(), REQUIRED.len() + 1);
    }

    #[test]
    fn reads_the_private_key_from_a_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", PrivateKey::new(SEED).encode()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut variables = without(Variable::PrivateKey);
        variables.insert(Variable::PrivateKeyFile, file.path().display().to_string());
        assert_eq!(load(&variables).unwrap().signer.key_id(), KEY_ID);
    }

    #[test]
    fn refuses_both_private_key_sources() {
        let problems = problems(&with(&[(Variable::PrivateKeyFile, "/run/secrets/key")]));
        assert!(
            matches!(
                problems.as_slice(),
                [Problem::PrivateKey(PrivateKeyError::Ambiguous)]
            ),
            "{problems:?}"
        );
    }

    #[test]
    fn refuses_neither_private_key_source() {
        let problems = problems(&without(Variable::PrivateKey));
        assert!(
            matches!(
                problems.as_slice(),
                [Problem::PrivateKey(PrivateKeyError::Missing)]
            ),
            "{problems:?}"
        );
    }

    #[test]
    fn refuses_a_key_id_the_keyring_does_not_hold() {
        let problems = problems(&with(&[(Variable::KeyId, "2099-1")]));
        assert!(
            matches!(
                problems.as_slice(),
                [Problem::Untrusted(Untrusted::UnknownKeyId { id })] if id == "2099-1"
            ),
            "{problems:?}"
        );
    }

    #[test]
    fn refuses_a_private_key_that_does_not_match_the_keyring() {
        let other = PrivateKey::new([12; 32]).encode();
        let problems = problems(&with(&[(Variable::PrivateKey, &other)]));
        assert!(
            matches!(
                problems.as_slice(),
                [Problem::Untrusted(Untrusted::PublicKeyMismatch { id })] if id == KEY_ID
            ),
            "{problems:?}"
        );
    }

    #[test]
    fn refuses_a_bad_listen_address() {
        for listen in ["8080", "localhost:8080", "0.0.0.0", "0.0.0.0:http"] {
            let problems = problems(&with(&[(Variable::Listen, listen)]));
            assert_eq!(invalid(&problems), [Variable::Listen], "{listen}");
        }
    }

    #[test]
    fn refuses_a_bad_smtp_port() {
        for port in ["submission", "0", "65536", "-1"] {
            let problems = problems(&with(&[(Variable::SmtpPort, port)]));
            assert_eq!(invalid(&problems), [Variable::SmtpPort], "{port}");
        }
    }

    #[test]
    fn refuses_a_sender_that_is_not_a_bare_address() {
        for from in ["licenses", "Zone <licenses@example.test>"] {
            let problems = problems(&with(&[(Variable::SmtpFrom, from)]));
            assert_eq!(invalid(&problems), [Variable::SmtpFrom], "{from}");
        }
    }

    #[test]
    fn refuses_a_sender_name_with_a_line_break() {
        let problems = problems(&with(&[(
            Variable::SmtpFromName,
            "Zone\r\nBcc: x@example.test",
        )]));
        assert_eq!(invalid(&problems), [Variable::SmtpFromName]);
    }

    #[test]
    fn reports_every_invalid_variable_together() {
        let problems = problems(&with(&[
            (Variable::Listen, "nowhere"),
            (Variable::SmtpPort, "smtp"),
        ]));
        assert_eq!(invalid(&problems), [Variable::Listen, Variable::SmtpPort]);
    }

    #[test]
    fn parses_the_product_list() {
        let config = load(&with(&[(Variable::Products, " prod_a, ,prod_b ,,prod_c,")])).unwrap();
        assert_eq!(
            config.products,
            Products::parse("prod_a,prod_b,prod_c").unwrap()
        );
        assert_eq!(config.products.len(), 3);
    }

    #[test]
    fn maps_each_product_to_its_plan() {
        let config = load(&with(&[(
            Variable::Products,
            "prod_a, prod_b = team, prod_c=enterprise",
        )]))
        .unwrap();
        assert_eq!(config.products.plan("prod_a"), Some(Plan::Enterprise));
        assert_eq!(config.products.plan("prod_b"), Some(Plan::Team));
        assert_eq!(config.products.plan("prod_c"), Some(Plan::Enterprise));
    }

    #[test]
    fn refuses_a_product_list_without_a_product() {
        for list in [",", " , ,"] {
            let problems = problems(&with(&[(Variable::Products, list)]));
            assert_eq!(invalid(&problems), [Variable::Products], "{list:?}");
        }
    }

    #[test]
    fn refuses_a_bad_product_entry_naming_it() {
        for (list, entry) in [
            ("prod_a,prod_b=business", "prod_b=business"),
            ("prod_a,=team", "=team"),
            ("prod_a,prod_a=team", "prod_a=team"),
        ] {
            let problems = problems(&with(&[(Variable::Products, list)]));
            assert_eq!(invalid(&problems), [Variable::Products], "{list}");
            let shown = problems[0].to_string();
            assert!(
                shown.starts_with("POLAR_PRODUCT_IDS is invalid: "),
                "{shown}"
            );
            assert!(shown.contains(&format!("{entry:?}")), "{shown}");
        }
    }

    #[test]
    fn values_are_trimmed() {
        let config = load(&with(&[
            (Variable::KeyId, " test-1 "),
            (Variable::SmtpHost, " smtp.example.test "),
            (Variable::SmtpFrom, " licenses@example.test "),
        ]))
        .unwrap();
        assert_eq!(config.signer.key_id(), KEY_ID);
        assert_eq!(config.smtp.host, "smtp.example.test");
        assert_eq!(config.smtp.sender.address, "licenses@example.test");
    }

    #[test]
    fn the_access_token_is_optional() {
        let config = load(&complete()).unwrap();
        assert_eq!(config.access_token, None);
    }

    #[test]
    fn a_blank_access_token_counts_as_unset() {
        for blank in ["", "  "] {
            let config = load(&with(&[(Variable::AccessToken, blank)])).unwrap();
            assert_eq!(config.access_token, None, "{blank:?}");
        }
    }

    #[test]
    fn reads_the_access_token_trimmed() {
        let config = load(&with(&[(
            Variable::AccessToken,
            &format!(" {ACCESS_TOKEN} \n"),
        )]))
        .unwrap();
        assert_eq!(config.access_token, Some(AccessToken::new(ACCESS_TOKEN)));
    }

    #[test]
    fn keeps_the_keyring_it_was_given() {
        let config = load(&complete()).unwrap();
        assert_eq!(config.keyring, keyring());
    }

    #[test]
    fn distribution_defaults_to_no_revocations_no_proxies_and_the_default_limits() {
        let config = load(&complete()).unwrap();
        assert!(config.revoked.is_empty());
        assert!(config.proxies.is_empty());
        assert_eq!(config.limits, Limits::default());
        assert_eq!(config.limits.address.get(), 120);
        assert_eq!(config.limits.key.get(), 240);
    }

    #[test]
    fn every_distribution_variable_is_honoured() {
        let config = load(&with(&[
            (
                Variable::Revoked,
                &format!("{REVOKED_UPPER} , ,{REVOKED_OTHER},"),
            ),
            (
                Variable::TrustedProxies,
                " 172.18.0.0/16, 10.0.0.5 ,fd00::/8",
            ),
            (Variable::AddressLimit, "30"),
            (Variable::KeyLimit, " 60 "),
            (
                Variable::RevokedSubscriptions,
                &format!(" {} ,", REVOKED_SUBSCRIPTION.to_uppercase()),
            ),
        ]))
        .unwrap();
        assert_eq!(
            config.revoked.licenses(),
            &HashSet::from([
                REVOKED_UPPER.parse().unwrap(),
                REVOKED_OTHER.parse().unwrap()
            ])
        );
        assert_eq!(
            config.revoked.subscriptions(),
            &HashSet::from([REVOKED_SUBSCRIPTION.parse().unwrap()])
        );
        assert_eq!(
            config.proxies.to_string(),
            "172.18.0.0/16,10.0.0.5/32,fd00::/8"
        );
        assert!(config.proxies.trusts("172.18.4.2".parse().unwrap()));
        assert!(!config.proxies.trusts("10.0.0.6".parse().unwrap()));
        assert_eq!(config.limits.address.get(), 30);
        assert_eq!(config.limits.key.get(), 60);
    }

    #[test]
    fn blank_distribution_variables_count_as_unset() {
        for blank in ["", "  "] {
            let config = load(&with(&[
                (Variable::Revoked, blank),
                (Variable::RevokedSubscriptions, blank),
                (Variable::TrustedProxies, blank),
                (Variable::AddressLimit, blank),
                (Variable::KeyLimit, blank),
            ]))
            .unwrap();
            assert!(config.revoked.is_empty(), "{blank:?}");
            assert!(config.proxies.is_empty(), "{blank:?}");
            assert_eq!(config.limits, Limits::default(), "{blank:?}");
        }
    }

    #[test]
    fn refuses_a_trusted_proxy_that_is_not_a_range_naming_it() {
        for entry in [
            "172.18.0.0/33",
            "proxy.internal",
            "10.0.0.0/8/1",
            "fd00::/129",
        ] {
            let problems = problems(&with(&[(
                Variable::TrustedProxies,
                &format!("172.18.0.0/16,{entry}"),
            )]));
            assert_eq!(invalid(&problems), [Variable::TrustedProxies], "{entry}");
            let shown = problems[0].to_string();
            assert!(
                shown.starts_with("ZONE_LICENSE_TRUSTED_PROXIES is invalid: "),
                "{shown}"
            );
            assert!(shown.contains(&format!("{entry:?}")), "{shown}");
        }
    }

    #[test]
    fn refuses_a_revoked_entry_that_is_not_a_uuid_naming_it() {
        for entry in ["not-a-uuid", "0193a8e2-7c4b-7d10-8f00", "42"] {
            let problems = problems(&with(&[(
                Variable::Revoked,
                &format!("{REVOKED_OTHER},{entry}"),
            )]));
            assert_eq!(invalid(&problems), [Variable::Revoked], "{entry}");
            let shown = problems[0].to_string();
            assert!(
                shown.starts_with("ZONE_LICENSE_REVOKED is invalid: "),
                "{shown}"
            );
            assert!(shown.contains(&format!("{entry:?}")), "{shown}");
        }
    }

    #[test]
    fn refuses_a_revoked_subscription_that_is_not_a_uuid_naming_it() {
        let problems = problems(&with(&[(
            Variable::RevokedSubscriptions,
            &format!("{REVOKED_SUBSCRIPTION},sub_01HZX0Q7M8"),
        )]));
        assert_eq!(invalid(&problems), [Variable::RevokedSubscriptions]);
        assert_eq!(
            problems[0].to_string(),
            "ZONE_LICENSE_REVOKED_SUBSCRIPTIONS is invalid: the entry \"sub_01HZX0Q7M8\" is not a subscription ID, which is a UUID"
        );
    }

    #[test]
    fn refuses_a_zero_or_unreadable_limit() {
        for variable in [Variable::AddressLimit, Variable::KeyLimit] {
            for limit in ["0", "-1", "ten", "1.5", "4294967296"] {
                let problems = problems(&with(&[(variable, limit)]));
                assert_eq!(invalid(&problems), [variable], "{variable:?} {limit}");
            }
        }
    }

    #[test]
    fn reports_every_invalid_distribution_variable_together() {
        let problems = problems(&with(&[
            (Variable::Revoked, "nobody"),
            (Variable::RevokedSubscriptions, "nothing"),
            (Variable::TrustedProxies, "everywhere"),
            (Variable::AddressLimit, "0"),
            (Variable::KeyLimit, "0"),
        ]));
        assert_eq!(
            invalid(&problems),
            [
                Variable::Revoked,
                Variable::RevokedSubscriptions,
                Variable::TrustedProxies,
                Variable::AddressLimit,
                Variable::KeyLimit,
            ]
        );
    }

    fn required(problems: &[Problem]) -> Vec<Variable> {
        problems
            .iter()
            .filter_map(|problem| match problem {
                Problem::Required { variable, .. } => Some(*variable),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_gated_route_without_trusted_proxies_refuses_to_start_naming_the_variable() {
        let registry = with(&[(Variable::RegistryService, "registry.example.test")]);
        let mut bundles = bundled(&[]);
        bundles.remove(&Variable::TrustedProxies);
        let mut both = bundles.clone();
        both.insert(
            Variable::RegistryService,
            "registry.example.test".to_owned(),
        );
        let mut blank = both.clone();
        blank.insert(Variable::TrustedProxies, "  ".to_owned());
        for variables in [registry, bundles, both, blank] {
            let problems = problems(&variables);
            assert_eq!(
                required(&problems),
                [Variable::TrustedProxies],
                "{problems:?}"
            );
            assert_eq!(problems.len(), 1, "{problems:?}");
            let shown = problems[0].to_string();
            assert!(
                shown.starts_with("ZONE_LICENSE_TRUSTED_PROXIES is not set: "),
                "{shown}"
            );
            assert!(shown.contains("or to none"), "{shown}");
        }
    }

    #[test]
    fn none_trusts_no_proxy_for_a_gated_route() {
        for none in ["none", " NONE ", "None"] {
            let config = load(&with(&[
                (Variable::RegistryService, "registry.example.test"),
                (Variable::TrustedProxies, none),
            ]))
            .unwrap();
            assert!(config.proxies.is_empty(), "{none:?}");
            assert!(config.registry.is_some(), "{none:?}");
            assert_eq!(config.proxies.to_string(), "none");
        }
        let config = load(&bundled(&[])).unwrap();
        assert!(config.proxies.is_empty());
        assert!(config.bundles.is_some());
    }

    #[test]
    fn none_is_refused_beside_a_range() {
        let problems = problems(&with(&[(Variable::TrustedProxies, "none,172.18.0.0/16")]));
        assert_eq!(invalid(&problems), [Variable::TrustedProxies]);
        assert!(problems[0].to_string().contains("\"none\""), "{problems:?}");
    }

    #[test]
    fn without_a_gated_route_trusted_proxies_stay_optional() {
        let config = load(&complete()).unwrap();
        assert!(config.registry.is_none());
        assert!(config.bundles.is_none());
        assert!(config.proxies.is_empty());
    }

    #[test]
    fn debug_redacts_the_access_token() {
        let config = load(&with(&[(Variable::AccessToken, ACCESS_TOKEN)])).unwrap();
        let shown = format!("{config:?} {config:#?}");
        assert!(!shown.contains(ACCESS_TOKEN), "{shown}");
        assert!(!shown.contains("config-test-token"), "{shown}");
        assert!(shown.contains("access_token"), "{shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
    }

    fn bundled(changes: &[(Variable, &str)]) -> HashMap<Variable, String> {
        let mut variables = with(&[
            (Variable::BundleBucket, BUCKET),
            (Variable::BundleRegion, REGION),
            (Variable::BundleAccessKeyId, ACCESS_KEY_ID),
            (Variable::BundleSecretAccessKey, SECRET_ACCESS_KEY),
            (Variable::TrustedProxies, NONE),
        ]);
        for (variable, value) in changes {
            variables.insert(*variable, (*value).to_owned());
        }
        variables
    }

    fn bundles(variables: &HashMap<Variable, String>) -> BundleSettings {
        load(variables).unwrap().bundles.expect("bundles on")
    }

    #[test]
    fn bundles_are_off_without_a_bucket() {
        assert!(load(&complete()).unwrap().bundles.is_none());
        let mut variables = bundled(&[(Variable::BundleBucket, "  ")]);
        assert!(load(&variables).unwrap().bundles.is_none());
        variables.remove(&Variable::BundleBucket);
        assert!(load(&variables).unwrap().bundles.is_none());
    }

    #[test]
    fn a_bucket_with_its_region_and_keys_turns_bundles_on() {
        let settings = bundles(&bundled(&[]));
        assert_eq!(settings.bucket.name(), BUCKET);
        assert_eq!(settings.bucket.region(), REGION);
        assert_eq!(
            settings.bucket.base_url().as_str(),
            "https://s3.ap-southeast-2.amazonaws.com/zone-distribution/"
        );
        assert_eq!(settings.prefix, "bundles");
        assert_eq!(settings.credentials.key(), ACCESS_KEY_ID);
        assert_eq!(settings.credentials.secret(), SECRET_ACCESS_KEY);
    }

    #[test]
    fn the_endpoint_default_follows_the_region() {
        let settings = bundles(&bundled(&[(Variable::BundleRegion, "eu-central-1")]));
        assert_eq!(
            settings.bucket.base_url().as_str(),
            "https://s3.eu-central-1.amazonaws.com/zone-distribution/"
        );
        assert_eq!(settings.bucket.region(), "eu-central-1");
    }

    #[test]
    fn the_endpoint_and_prefix_are_honoured_path_style() {
        let settings = bundles(&bundled(&[
            (Variable::BundleEndpoint, " http://127.0.0.1:9000/s3 "),
            (Variable::BundlePrefix, "/zone/bundles/"),
        ]));
        assert_eq!(
            settings.bucket.base_url().as_str(),
            "http://127.0.0.1:9000/s3/zone-distribution/"
        );
        assert_eq!(settings.bucket.region(), REGION);
        assert_eq!(settings.prefix, "zone/bundles");
    }

    #[test]
    fn a_bucket_alone_names_each_missing_bundle_variable() {
        let problems = problems(&with(&[
            (Variable::BundleBucket, BUCKET),
            (Variable::TrustedProxies, NONE),
        ]));
        let missing: Vec<Variable> = problems
            .iter()
            .filter_map(|problem| match problem {
                Problem::Missing(variable) => Some(*variable),
                _ => None,
            })
            .collect();
        assert_eq!(
            missing,
            [
                Variable::BundleRegion,
                Variable::BundleAccessKeyId,
                Variable::BundleSecretAccessKey,
            ],
            "{problems:?}"
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn each_missing_bundle_variable_is_named_alone() {
        for missing in [
            Variable::BundleRegion,
            Variable::BundleAccessKeyId,
            Variable::BundleSecretAccessKey,
        ] {
            let mut variables = bundled(&[]);
            variables.remove(&missing);
            let problems = problems(&variables);
            assert!(
                matches!(problems.as_slice(), [Problem::Missing(variable)] if *variable == missing),
                "{missing:?}: {problems:?}"
            );
            let message = load(&variables).expect_err("refused").to_string();
            assert!(message.contains(missing.name()), "{message}");
        }
    }

    #[test]
    fn refuses_a_bad_bundle_region() {
        for region in ["AP-SOUTHEAST-2", "ap southeast 2", "evil.example/", "a:1"] {
            let problems = problems(&bundled(&[(Variable::BundleRegion, region)]));
            assert_eq!(invalid(&problems), [Variable::BundleRegion], "{region}");
        }
    }

    #[test]
    fn refuses_a_bad_bundle_endpoint() {
        for endpoint in [
            "s3.amazonaws.com",
            "ftp://s3.amazonaws.com",
            "https://",
            "not a url",
        ] {
            let problems = problems(&bundled(&[(Variable::BundleEndpoint, endpoint)]));
            assert_eq!(invalid(&problems), [Variable::BundleEndpoint], "{endpoint}");
        }
    }

    #[test]
    fn refuses_a_bad_bundle_prefix() {
        for prefix in ["/", "a/../b", "a//b", "a b"] {
            let problems = problems(&bundled(&[(Variable::BundlePrefix, prefix)]));
            assert_eq!(invalid(&problems), [Variable::BundlePrefix], "{prefix}");
        }
    }

    #[test]
    fn a_partial_bundle_config_reports_every_problem_at_once() {
        let mut variables = with(&[
            (Variable::BundleBucket, BUCKET),
            (Variable::BundleEndpoint, "ftp://nowhere"),
            (Variable::BundlePrefix, ".."),
            (Variable::BundleAccessKeyId, ACCESS_KEY_ID),
        ]);
        variables.remove(&Variable::SmtpHost);
        let problems = problems(&variables);
        assert_eq!(
            invalid(&problems),
            [Variable::BundleEndpoint, Variable::BundlePrefix]
        );
        let shown = ConfigError::new(problems).to_string();
        for variable in [
            Variable::SmtpHost,
            Variable::BundleRegion,
            Variable::BundleSecretAccessKey,
            Variable::BundleEndpoint,
            Variable::BundlePrefix,
        ] {
            assert!(shown.contains(variable.name()), "{shown}");
        }
    }

    #[test]
    fn debug_hides_the_bundle_secret() {
        let config = load(&bundled(&[])).unwrap();
        let shown = format!("{config:?} {config:#?}");
        assert!(!shown.contains(SECRET_ACCESS_KEY), "{shown}");
        assert!(!shown.contains("hidden+key"), "{shown}");
        assert!(shown.contains(ACCESS_KEY_ID), "{shown}");
        assert!(shown.contains(BUCKET), "{shown}");
        assert!(shown.contains("secret_access_key"), "{shown}");
    }

    #[test]
    fn debug_redacts_the_secrets() {
        let config = load(&complete()).unwrap();
        let shown = format!("{config:?} {config:#?}");
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(!shown.contains("config-test-secret"), "{shown}");
        assert!(!shown.contains(&PrivateKey::new(SEED).encode()), "{shown}");
        assert!(!shown.contains(PASSWORD), "{shown}");
        assert!(shown.contains(KEY_ID), "{shown}");
        assert!(shown.contains("smtp.example.test"), "{shown}");
    }

    fn registry(changes: &[(Variable, &str)]) -> Option<RegistrySettings> {
        let mut variables = with(changes);
        variables
            .entry(Variable::TrustedProxies)
            .or_insert_with(|| NONE.to_owned());
        load(&variables).unwrap().registry
    }

    #[test]
    fn the_registry_is_off_without_a_service() {
        assert_eq!(load(&complete()).unwrap().registry, None);
        assert_eq!(
            registry(&[
                (Variable::RegistryRepositories, "zone/server"),
                (Variable::RegistryKeyGeneration, "2"),
            ]),
            None
        );
    }

    #[test]
    fn a_blank_registry_service_counts_as_unset() {
        for blank in ["", "  "] {
            assert_eq!(
                registry(&[(Variable::RegistryService, blank)]),
                None,
                "{blank:?}"
            );
        }
    }

    #[test]
    fn a_registry_service_alone_configures_the_registry_defaults() {
        let settings = registry(&[(Variable::RegistryService, " registry.bitecode.ltd ")]).unwrap();
        assert_eq!(settings.service, "registry.bitecode.ltd");
        assert_eq!(settings.generation, DEFAULT_REGISTRY_KEY_GENERATION);
        assert_eq!(settings.generation.get(), 1);
        assert_eq!(settings.repositories, Repositories::default());
        assert_eq!(
            settings.repositories.iter().collect::<Vec<_>>(),
            ["zone/server", "zone/manager", "zone/litellm"]
        );
    }

    #[test]
    fn every_registry_variable_is_honoured() {
        let settings = registry(&[
            (Variable::RegistryService, "registry.example.test"),
            (
                Variable::RegistryRepositories,
                " zone/server , ,zone/runner,",
            ),
            (Variable::RegistryKeyGeneration, "3"),
        ])
        .unwrap();
        assert_eq!(settings.service, "registry.example.test");
        assert_eq!(
            settings.repositories.iter().collect::<Vec<_>>(),
            ["zone/server", "zone/runner"]
        );
        assert_eq!(settings.generation.get(), 3);
    }

    #[test]
    fn a_blank_repository_list_means_the_default() {
        let settings = registry(&[
            (Variable::RegistryService, "registry.example.test"),
            (Variable::RegistryRepositories, "  "),
        ])
        .unwrap();
        assert_eq!(settings.repositories, Repositories::default());
    }

    #[test]
    fn refuses_a_registry_key_generation_below_one() {
        for generation in ["0", "-1", "one", "1.5", "4294967296"] {
            let problems = problems(&with(&[
                (Variable::RegistryService, "registry.example.test"),
                (Variable::RegistryKeyGeneration, generation),
            ]));
            assert_eq!(
                invalid(&problems),
                [Variable::RegistryKeyGeneration],
                "{generation}"
            );
        }
    }

    #[test]
    fn refuses_a_repository_list_without_a_repository() {
        for list in [",", " , ,"] {
            let problems = problems(&with(&[
                (Variable::RegistryService, "registry.example.test"),
                (Variable::RegistryRepositories, list),
            ]));
            assert_eq!(
                invalid(&problems),
                [Variable::RegistryRepositories],
                "{list:?}"
            );
            assert_eq!(
                problems[0].to_string(),
                "ZONE_REGISTRY_REPOSITORIES is invalid: it names no repository"
            );
        }
    }

    #[test]
    fn refuses_a_bad_repository_entry_naming_it() {
        for (list, entry) in [
            ("zone/server,Zone/Manager", "Zone/Manager"),
            ("zone/server,zone//manager", "zone//manager"),
            (
                "zone/server,registry.example.test:5000/zone/server",
                "registry.example.test:5000/zone/server",
            ),
            ("zone/server,zone/server", "zone/server"),
        ] {
            let problems = problems(&with(&[
                (Variable::RegistryService, "registry.example.test"),
                (Variable::RegistryRepositories, list),
            ]));
            assert_eq!(
                invalid(&problems),
                [Variable::RegistryRepositories],
                "{list}"
            );
            assert!(
                problems[0].to_string().contains(&format!("{entry:?}")),
                "{}",
                problems[0]
            );
        }
    }

    #[test]
    fn checks_the_registry_variables_even_while_the_registry_is_off() {
        let problems = problems(&with(&[
            (Variable::RegistryRepositories, ","),
            (Variable::RegistryKeyGeneration, "0"),
        ]));
        assert_eq!(
            invalid(&problems),
            [
                Variable::RegistryRepositories,
                Variable::RegistryKeyGeneration
            ]
        );
    }

    #[test]
    fn refuses_a_registry_service_with_a_space() {
        for service in ["registry example", "registry.example.test\tx"] {
            let problems = problems(&with(&[(Variable::RegistryService, service)]));
            assert_eq!(
                invalid(&problems),
                [Variable::RegistryService],
                "{service:?}"
            );
        }
    }

    #[test]
    fn debug_shows_the_registry_settings() {
        let config = load(&with(&[
            (Variable::RegistryService, "registry.example.test"),
            (Variable::TrustedProxies, NONE),
        ]))
        .unwrap();
        let shown = format!("{config:?}");
        assert!(shown.contains("registry.example.test"), "{shown}");
        assert!(shown.contains("zone/litellm"), "{shown}");
    }
}

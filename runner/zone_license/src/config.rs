use std::fmt;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;

use abnegate_notify::{Sender, SmtpConfig};
use lettre::Address;
use zone_enterprise::Keyring;

use crate::config_error::ConfigError;
use crate::polar::WebhookSecret;
use crate::private_key::PrivateKey;
use crate::problem::Problem;
use crate::signer::Signer;
use crate::variable::Variable;

pub const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 8080));
pub const DEFAULT_SMTP_PORT: u16 = 587;
pub const DEFAULT_SENDER_NAME: &str = "Zone";

const PRODUCT_SEPARATOR: char = ',';
const LISTEN_REASON: &str = "it must be an IP address and port, such as 0.0.0.0:8080";
const PORT_REASON: &str = "it must be a port number from 1 to 65535";
const FROM_REASON: &str = "it must be a bare email address, such as licenses@example.com";
const FROM_NAME_REASON: &str = "it must be a single line";
const PRODUCTS_REASON: &str = "it must list at least one Polar product ID, separated by commas";

pub struct Config {
    pub signer: Signer,
    pub secret: WebhookSecret,
    pub products: Vec<String>,
    pub listen: SocketAddr,
    pub smtp: SmtpConfig,
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
                let products = products(&list);
                if products.is_empty() {
                    problems.push(invalid(Variable::Products, PRODUCTS_REASON));
                    return None;
                }
                Some(products)
            });
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

        let assembled = (|| {
            Some(Self {
                signer: signer?,
                secret: WebhookSecret::new(&secret?),
                products: products?,
                listen: listen?,
                smtp: SmtpConfig::new(
                    host?,
                    port?,
                    user?,
                    password?,
                    Sender::new(from?).with_name(name?),
                ),
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
            .field("listen", &self.listen)
            .field("smtp_host", &self.smtp.host)
            .field("smtp_port", &self.smtp.port)
            .field("smtp_user", &self.smtp.user)
            .field("smtp_from", &self.smtp.sender.address)
            .field("smtp_from_name", &self.smtp.sender.name)
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

fn products(list: &str) -> Vec<String> {
    list.split(PRODUCT_SEPARATOR)
        .map(str::trim)
        .filter(|product| !product.is_empty())
        .map(str::to_owned)
        .collect()
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
    use std::collections::HashMap;
    use std::io::Write;

    use zone_enterprise::TrustedKey;

    use super::*;
    use crate::private_key_error::PrivateKeyError;
    use crate::untrusted::Untrusted;

    const SEED: [u8; 32] = [11; 32];
    const KEY_ID: &str = "test-1";
    const SECRET: &str = "whsec_config-test-secret";
    const PASSWORD: &str = "hunter2-not-a-real-password";
    const PRODUCT: &str = "prod_a";
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
        assert_eq!(config.products, [PRODUCT]);
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
        assert_eq!(config.products, ["prod_a", "prod_b", "prod_c"]);
    }

    #[test]
    fn refuses_a_product_list_without_a_product() {
        for list in [",", " , ,"] {
            let problems = problems(&with(&[(Variable::Products, list)]));
            assert_eq!(invalid(&problems), [Variable::Products], "{list:?}");
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
}

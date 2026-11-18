use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Datelike, Utc};
use zone_enterprise::Keyring;

use crate::keygen_arguments::KeygenArguments;
use crate::keygen_error::KeygenError;
use crate::private_key::{PrivateKey, SEED_LENGTH};
use crate::variable::Variable;

const BYTES_PER_LINE: usize = 15;
const KEY_ID_SEPARATOR: char = '-';
const PRODUCTION_SOURCE: &str = "runner/zone_enterprise/src/production.rs";

pub fn run(
    arguments: &KeygenArguments,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), KeygenError> {
    let key = generate()?;
    let key_id = arguments
        .key_id
        .clone()
        .unwrap_or_else(|| suggest(&Keyring::production(), Utc::now().year()));
    let report: &mut dyn Write = match &arguments.output {
        Some(path) => {
            write_new(path, &key)?;
            writeln!(stdout, "Wrote the private key to {}.", path.display())?;
            stdout
        }
        None => {
            writeln!(stdout, "{}", key.encode())?;
            stderr
        }
    };
    let public = key.public();
    writeln!(report, "Key id: {key_id}")?;
    writeln!(report, "Public key (base64): {}", STANDARD.encode(public))?;
    writeln!(
        report,
        "Trust it by adding this entry to KEYS in {PRODUCTION_SOURCE}:"
    )?;
    writeln!(report, "{},", entry(&key_id, &public))?;
    writeln!(
        report,
        "Sign with {}={key_id} once a release carries it.",
        Variable::KeyId.name()
    )?;
    Ok(())
}

pub fn generate() -> Result<PrivateKey, KeygenError> {
    let mut seed = [0; SEED_LENGTH];
    getrandom::fill(&mut seed)?;
    Ok(PrivateKey::new(seed))
}

pub fn suggest(keyring: &Keyring, year: i32) -> String {
    let prefix = format!("{year}{KEY_ID_SEPARATOR}");
    let highest = keyring
        .ids()
        .filter_map(|id| id.strip_prefix(&prefix)?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", highest + 1)
}

pub fn entry(key_id: &str, public: &[u8; 32]) -> String {
    let lines: Vec<String> = public
        .chunks(BYTES_PER_LINE)
        .map(|chunk| {
            let bytes: Vec<String> = chunk.iter().map(|byte| format!("{byte:#04x}")).collect();
            format!("        {},", bytes.join(", "))
        })
        .collect();
    format!(
        "(\n    \"{key_id}\",\n    [\n{}\n    ],\n)",
        lines.join("\n")
    )
}

fn write_new(path: &Path, key: &PrivateKey) -> Result<(), KeygenError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|source| match source.kind() {
        io::ErrorKind::AlreadyExists => KeygenError::Exists {
            path: path.to_path_buf(),
        },
        _ => KeygenError::Write {
            path: path.to_path_buf(),
            source,
        },
    })?;
    writeln!(file, "{}", key.encode())
        .and_then(|()| file.sync_all())
        .map_err(|source| {
            let _ = fs::remove_file(path);
            KeygenError::Write {
                path: path.to_path_buf(),
                source,
            }
        })
}

#[cfg(test)]
mod tests {
    use zone_enterprise::TrustedKey;

    use super::*;

    const PRODUCTION: &str = include_str!("../../zone_enterprise/src/production.rs");

    fn listed_bytes(entry: &str) -> Vec<u8> {
        entry
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("0x"))
            .flat_map(|line| line.split_terminator(','))
            .map(|token| {
                let digits = token.trim().strip_prefix("0x").expect("a 0x byte");
                assert!(
                    digits.len() == 2 && digits.bytes().all(|digit| digit.is_ascii_hexdigit()),
                    "{token:?} is not a 0xNN byte"
                );
                u8::from_str_radix(digits, 16).unwrap()
            })
            .collect()
    }

    fn run_to_strings(arguments: &KeygenArguments) -> (String, String) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        run(arguments, &mut stdout, &mut stderr).unwrap();
        (
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn suggests_the_next_free_id_for_this_year_after_production() {
        let production = Keyring::production();
        let year = Utc::now().year();
        let suggested = suggest(&production, year);
        let (prefix, number) = suggested.split_once(KEY_ID_SEPARATOR).unwrap();
        let number: u32 = number.parse().unwrap();
        assert_eq!(prefix, year.to_string());
        assert!(production.get(&suggested).is_none(), "{suggested} is taken");
        assert!(
            number == 1 || production.get(&format!("{year}-{}", number - 1)).is_some(),
            "{suggested} skips a free id"
        );
    }

    #[test]
    fn suggests_the_first_id_of_a_year_production_has_no_key_for() {
        let year = Keyring::production()
            .ids()
            .filter_map(|id| id.split_once(KEY_ID_SEPARATOR)?.0.parse::<i32>().ok())
            .max()
            .unwrap()
            + 1;
        assert_eq!(suggest(&Keyring::production(), year), format!("{year}-1"));
    }

    #[test]
    fn suggests_past_the_highest_numbered_id() {
        let keys = ["2026-1", "2026-10", "2026-3", "2026-x", "20261-4"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| {
                TrustedKey::new(id, PrivateKey::new([index as u8 + 1; 32]).public()).unwrap()
            });
        assert_eq!(suggest(&Keyring::new(keys).unwrap(), 2026), "2026-11");
    }

    #[test]
    fn renders_production_rs_exactly() {
        let public = Keyring::production().get("2026-1").unwrap().bytes();
        let rendered = entry("2026-1", &public);
        assert!(PRODUCTION.contains(&rendered), "{rendered}");
    }

    #[test]
    fn the_rendered_array_round_trips() {
        let public = PrivateKey::new([9; 32]).public();
        assert_eq!(listed_bytes(&entry("2026-2", &public)), public);
    }

    #[test]
    fn the_array_is_read_apart_from_a_0x_elsewhere_in_the_report() {
        let public = PrivateKey::new([9; 32]).public();
        let report = format!(
            "Wrote the private key to /tmp/0xq9/signing.key.\n\
             Public key (base64): Ab0xZf+=\n\
             {}\n",
            entry("2026-2", &public)
        );
        assert_eq!(listed_bytes(&report), public);
    }

    #[test]
    fn generates_distinct_keys() {
        assert_ne!(generate().unwrap().encode(), generate().unwrap().encode());
    }

    #[test]
    fn prints_the_private_key_alone_on_stdout() {
        let (stdout, stderr) = run_to_strings(&KeygenArguments {
            key_id: Some("2026-2".into()),
            ..KeygenArguments::default()
        });
        assert_eq!(stdout.lines().count(), 1, "{stdout}");
        let key = PrivateKey::parse(&stdout).unwrap();
        assert!(stderr.contains(&entry("2026-2", &key.public())), "{stderr}");
        assert!(stderr.contains(&STANDARD.encode(key.public())), "{stderr}");
        assert!(
            !stderr.contains(&key.encode()),
            "the report leaked the private key"
        );
    }

    #[test]
    fn suggests_a_key_id_when_none_is_given() {
        let (_, stderr) = run_to_strings(&KeygenArguments::default());
        let expected = suggest(&Keyring::production(), Utc::now().year());
        assert!(stderr.contains(&format!("Key id: {expected}")), "{stderr}");
    }

    #[test]
    fn writes_a_new_owner_only_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("signing.key");
        let (stdout, stderr) = run_to_strings(&KeygenArguments {
            output: Some(path.clone()),
            key_id: Some("2026-2".into()),
        });
        assert_eq!(stderr, "");
        let key = PrivateKey::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(stdout.contains(&entry("2026-2", &key.public())), "{stdout}");
        assert!(
            !stdout.contains(&key.encode()),
            "stdout leaked the private key"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(PrivateKey::resolve(Some(&path), None).is_ok());
    }

    #[test]
    fn refuses_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("signing.key");
        fs::write(&path, "keep me").unwrap();
        let error = run(
            &KeygenArguments {
                output: Some(path.clone()),
                key_id: None,
            },
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(error, KeygenError::Exists { path: found } if found == path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
    }
}

//! How a configuration error reads to a person.

use std::error::Error as _;
use std::fmt;

use abnegate_config::Error;

/// How toml begins a report that it can place in the file.
const POSITION_PREFIX: &str = "TOML parse error at ";

const QUOTE: char = '"';

const ELIDED: &str = "…";

/// A configuration error followed by its causes. A parse error's own report
/// quotes the offending line of the file, and a type error quotes the value,
/// either of which can be a credential, so a parse error keeps only where the
/// file was rejected and why, with any quoted value elided.
pub struct Explanation<'a>(pub &'a Error);

impl fmt::Display for Explanation<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)?;

        if let Error::Parse { source, .. } = self.0 {
            let report = source.to_string();
            if let Some(position) = report
                .lines()
                .next()
                .and_then(|line| line.strip_prefix(POSITION_PREFIX))
            {
                write!(formatter, " at {position}")?;
            }
            return write!(formatter, ": {}", elided(source.message()));
        }

        let mut cause = self.0.source();
        while let Some(source) = cause {
            write!(formatter, ": {source}")?;
            cause = source.source();
        }
        Ok(())
    }
}

fn elided(message: &str) -> String {
    message
        .lines()
        .collect::<Vec<_>>()
        .join("; ")
        .split(QUOTE)
        .enumerate()
        .map(|(index, part)| if index % 2 == 1 { ELIDED } else { part })
        .collect::<Vec<_>>()
        .join(&QUOTE.to_string())
}

#[cfg(test)]
mod tests {
    use abnegate_config::Loader;

    use super::*;
    use crate::config::Config;

    fn explained(contents: &str) -> (String, String) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, contents).unwrap();

        let error = Loader::at(&path).load::<Config>().unwrap_err();

        (Explanation(&error).to_string(), path.display().to_string())
    }

    #[test]
    fn a_parse_error_names_its_place_but_never_quotes_the_line() {
        let (explanation, path) = explained("model = \"gpt-4o\"\nllm_api_key = \"sk-secret\n");

        assert!(
            explanation.starts_with(&format!("Failed to parse '{path}' at line 2, column ")),
            "{explanation}"
        );
        assert!(!explanation.contains("sk-secret"), "{explanation}");
        assert!(!explanation.contains('\n'), "{explanation}");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_never_quoted() {
        let (explanation, path) = explained("max_iterations = \"sk-secret\"\n");

        assert!(
            explanation.starts_with(&format!("Failed to parse '{path}'")),
            "{explanation}"
        );
        assert!(!explanation.contains("sk-secret"), "{explanation}");
    }

    #[test]
    fn any_other_error_carries_its_cause() {
        let error = Error::write(
            "/zone/sessions",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );

        assert_eq!(
            Explanation(&error).to_string(),
            "Failed to write '/zone/sessions': denied"
        );
    }
}

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Login(String);

impl Login {
    pub const MAXIMUM_LENGTH: usize = 39;

    pub fn parse(value: &str) -> Option<Login> {
        let valid = (1..=Self::MAXIMUM_LENGTH).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("--");
        valid.then(|| Login(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Login {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_is_what_github_accepts_as_an_account_name() {
        let longest = "a".repeat(Login::MAXIMUM_LENGTH);
        let too_long = "a".repeat(Login::MAXIMUM_LENGTH + 1);
        let cases: [(&str, bool); 16] = [
            ("octocat", true),
            ("a", true),
            ("9", true),
            ("Octo-Cat", true),
            ("my-org-2", true),
            (&longest, true),
            ("", false),
            (&too_long, false),
            ("-octocat", false),
            ("octocat-", false),
            ("octo--cat", false),
            ("octo_cat", false),
            ("octo cat", false),
            ("octo/cat", false),
            ("octo.cat", false),
            ("öctocat", false),
        ];
        for (value, accepted) in cases {
            assert_eq!(
                Login::parse(value).is_some(),
                accepted,
                "{value:?} should be {}",
                if accepted { "accepted" } else { "refused" }
            );
        }
    }

    #[test]
    fn a_login_keeps_its_value() {
        let login = Login::parse("Octo-Cat").expect("a valid login");
        assert_eq!(login.as_str(), "Octo-Cat");
        assert_eq!(login.to_string(), "Octo-Cat");
    }
}

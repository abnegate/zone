use crate::sso::error::Error;

const MAXIMUM_LENGTH: usize = 253;
const MAXIMUM_LABEL_LENGTH: usize = 63;
const SEPARATOR: char = '.';
const HYPHEN: char = '-';
const AT: char = '@';

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Domains(Vec<String>);

impl Domains {
    pub fn parse<I, S>(entries: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut domains: Vec<String> = Vec::new();
        for entry in entries {
            let domain = normalize(entry.as_ref())?;
            if !domains.contains(&domain) {
                domains.push(domain);
            }
        }
        Ok(Self(domains))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn contains(&self, domain: &str) -> bool {
        let domain = domain.trim().to_ascii_lowercase();
        self.0.contains(&domain)
    }

    pub fn allows(&self, email: &str) -> bool {
        if self.is_empty() {
            return true;
        }
        email
            .rsplit_once(AT)
            .is_some_and(|(_, domain)| self.contains(domain))
    }
}

fn normalize(entry: &str) -> Result<String, Error> {
    let domain = entry.trim().to_ascii_lowercase();
    if valid(&domain) {
        Ok(domain)
    } else {
        Err(Error::DomainInvalid(entry.trim().to_owned()))
    }
}

fn valid(domain: &str) -> bool {
    !domain.is_empty() && domain.len() <= MAXIMUM_LENGTH && domain.split(SEPARATOR).all(label)
}

fn label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAXIMUM_LABEL_LENGTH
        && !label.starts_with(HYPHEN)
        && !label.ends_with(HYPHEN)
        && label.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == HYPHEN
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_trims_and_dedupes() {
        let domains = Domains::parse([" Acme.com ", "acme.COM", "eu.acme.com"]).unwrap();
        assert_eq!(domains.as_slice(), ["acme.com", "eu.acme.com"]);
    }

    #[test]
    fn accepts_single_labels_digits_and_hyphens() {
        let domains = Domains::parse(["corp", "acme-labs.co.uk", "1password.com"]).unwrap();
        assert_eq!(domains.as_slice().len(), 3);
    }

    #[test]
    fn refuses_invalid_entries_and_names_them() {
        for entry in [
            "",
            "  ",
            "user@acme.com",
            "*.acme.com",
            "acme..com",
            ".acme.com",
            "acme.com.",
            "-acme.com",
            "acme-.com",
            "acme_com",
            "acme com",
            "https://acme.com",
            "bücher.de",
            &format!("{}.com", "a".repeat(MAXIMUM_LABEL_LENGTH + 1)),
            &format!("{}a", "a.".repeat(MAXIMUM_LENGTH / 2 + 1)),
        ] {
            assert_eq!(
                Domains::parse([entry]),
                Err(Error::DomainInvalid(entry.trim().to_owned())),
                "{entry:?}"
            );
        }
    }

    #[test]
    fn empty_allows_every_email() {
        let domains = Domains::parse(Vec::<String>::new()).unwrap();
        assert!(domains.is_empty());
        assert!(domains.allows("anyone@anywhere.test"));
    }

    #[test]
    fn allows_only_listed_domains_ignoring_case() {
        let domains = Domains::parse(["acme.com"]).unwrap();
        assert!(domains.allows("jane@acme.com"));
        assert!(domains.allows("Jane@ACME.Com"));
        assert!(!domains.allows("jane@evil.test"));
        assert!(!domains.allows("jane@eu.acme.com"));
        assert!(!domains.allows("jane@acme.com.evil.test"));
        assert!(!domains.allows("acme.com"));
    }

    #[test]
    fn uses_the_domain_after_the_last_at() {
        let domains = Domains::parse(["acme.com"]).unwrap();
        assert!(domains.allows("\"jane@evil.test\"@acme.com"));
        assert!(!domains.allows("jane@acme.com@evil.test"));
    }

    #[test]
    fn contains_ignores_case_and_whitespace() {
        let domains = Domains::parse(["acme.com"]).unwrap();
        assert!(domains.contains(" ACME.com"));
        assert!(!domains.contains("evil.test"));
    }
}

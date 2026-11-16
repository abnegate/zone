use super::flag::Flag;

pub struct Environment<Lookup> {
    lookup: Lookup,
}

impl<Lookup> Environment<Lookup>
where
    Lookup: Fn(&str) -> Option<String>,
{
    pub fn new(lookup: Lookup) -> Self {
        Self { lookup }
    }

    pub fn read(&self, variable: &str) -> Option<String> {
        (self.lookup)(variable).filter(|value| !value.trim().is_empty())
    }

    pub fn flag(&self, variable: &str) -> Option<bool> {
        let flag = Flag::parse(&self.read(variable)?);
        if let Some(warning) = flag.warning(variable) {
            tracing::warn!(variable, "{warning}");
        }
        Some(flag.enabled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLE: &str = "ZONE_ENVIRONMENT_TEST";

    fn holding(value: &'static str) -> Environment<impl Fn(&str) -> Option<String>> {
        Environment::new(move |variable| (variable == VARIABLE).then(|| value.to_string()))
    }

    #[test]
    fn blank_values_read_as_unset() {
        for blank in ["", " ", "\t", "\n", " \t\r\n "] {
            assert_eq!(
                holding(blank).read(VARIABLE),
                None,
                "{blank:?} must read as unset"
            );
        }
    }

    #[test]
    fn missing_variables_read_as_unset() {
        assert_eq!(holding("value").read("ZONE_ENVIRONMENT_OTHER"), None);
    }

    #[test]
    fn non_blank_values_are_read_as_written() {
        assert_eq!(holding("daily").read(VARIABLE).as_deref(), Some("daily"));
        assert_eq!(
            holding(" 45 ").read(VARIABLE).as_deref(),
            Some(" 45 "),
            "padding is left for each setting's parser to handle"
        );
    }

    #[test]
    fn a_flag_is_on_only_for_an_accepted_spelling_of_on() {
        for on in ["1", "true", "TRUE", "yes", "Yes", " on "] {
            assert_eq!(
                holding(on).flag(VARIABLE),
                Some(true),
                "{on:?} must turn on"
            );
        }
        for off in ["0", "false", "no", "off", "ture", "maybe"] {
            assert_eq!(
                holding(off).flag(VARIABLE),
                Some(false),
                "{off:?} must turn off, an unrecognised spelling included"
            );
        }
    }

    #[test]
    fn a_blank_or_missing_flag_is_unset() {
        assert_eq!(holding(" ").flag(VARIABLE), None);
        assert_eq!(holding("true").flag("ZONE_ENVIRONMENT_OTHER"), None);
    }
}

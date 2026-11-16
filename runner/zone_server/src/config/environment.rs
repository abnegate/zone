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
}

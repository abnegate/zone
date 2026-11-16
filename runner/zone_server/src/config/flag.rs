const ON: [&str; 4] = ["1", "true", "yes", "on"];
const OFF: [&str; 4] = ["0", "false", "no", "off"];

/// An on/off setting as the operator wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flag {
    On,
    Off,
    Unrecognised(String),
}

impl Flag {
    pub fn parse(value: &str) -> Self {
        let spelling = value.trim().to_ascii_lowercase();
        if ON.contains(&spelling.as_str()) {
            Self::On
        } else if OFF.contains(&spelling.as_str()) {
            Self::Off
        } else {
            Self::Unrecognised(value.to_string())
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self, Self::On)
    }

    /// Why `variable` holding this value is read as off although the operator
    /// may have meant it as on, if it is.
    pub fn warning(&self, variable: &str) -> Option<String> {
        match self {
            Self::Unrecognised(value) => Some(format!(
                "{variable} is {value:?}, which is neither on ({}) nor off ({}); reading it as off",
                ON.join(", "),
                OFF.join(", "),
            )),
            Self::On | Self::Off => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIABLE: &str = "ZONE_FLAG_TEST";

    #[test]
    fn each_accepted_spelling_reads_as_on_or_off_in_any_case_and_padding() {
        for on in ["1", "true", "TRUE", "yes", "Yes", " on "] {
            assert_eq!(Flag::parse(on), Flag::On, "{on:?} must turn on");
        }
        for off in ["0", "false", "False", "no", " OFF "] {
            assert_eq!(Flag::parse(off), Flag::Off, "{off:?} must turn off");
        }
    }

    #[test]
    fn an_unrecognised_spelling_is_off_and_says_which_spellings_are_accepted() {
        for typo in ["ture", "maybe", "2", "enabled"] {
            let flag = Flag::parse(typo);
            assert_eq!(flag, Flag::Unrecognised(typo.to_string()));
            assert!(!flag.enabled(), "{typo:?} must not turn on");
            let warning = flag
                .warning(VARIABLE)
                .unwrap_or_else(|| panic!("{typo:?} must be reported"));
            assert!(warning.contains(VARIABLE), "{warning}");
            assert!(warning.contains(&format!("{typo:?}")), "{warning}");
            for spelling in ON.iter().chain(OFF.iter()) {
                assert!(warning.contains(spelling), "{warning} omits {spelling}");
            }
        }
    }

    #[test]
    fn an_accepted_spelling_is_not_reported() {
        assert_eq!(Flag::On.warning(VARIABLE), None);
        assert_eq!(Flag::Off.warning(VARIABLE), None);
    }
}

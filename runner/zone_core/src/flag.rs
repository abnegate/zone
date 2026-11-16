const ON: [&str; 4] = ["1", "true", "yes", "on"];
const OFF: [&str; 4] = ["0", "false", "no", "off"];

/// An on/off setting as the operator wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flag {
    On,
    Off,
    Unset,
    Unrecognised(String),
}

impl Flag {
    pub fn parse(value: &str) -> Self {
        let spelling = value.trim().to_ascii_lowercase();
        if spelling.is_empty() {
            Self::Unset
        } else if ON.contains(&spelling.as_str()) {
            Self::On
        } else if OFF.contains(&spelling.as_str()) {
            Self::Off
        } else {
            Self::Unrecognised(value.to_string())
        }
    }

    /// `variable` holding `value`, or `None` when it is unset, blank or
    /// unrecognised so the caller's default applies. An unrecognised value is
    /// logged, since the operator meant something by it.
    pub fn read(variable: &str, value: Option<&str>) -> Option<bool> {
        let flag = value.map_or(Self::Unset, Self::parse);
        if let Some(warning) = flag.warning(variable) {
            tracing::warn!(variable, "{warning}");
        }
        flag.enabled()
    }

    pub fn environment(variable: &str, default: bool) -> bool {
        Self::read(variable, std::env::var(variable).ok().as_deref()).unwrap_or(default)
    }

    pub fn enabled(&self) -> Option<bool> {
        match self {
            Self::On => Some(true),
            Self::Off => Some(false),
            Self::Unset | Self::Unrecognised(_) => None,
        }
    }

    pub fn warning(&self, variable: &str) -> Option<String> {
        match self {
            Self::Unrecognised(value) => Some(format!(
                "{variable} is {value:?}, which is neither on ({}) nor off ({}); using its default",
                ON.join(", "),
                OFF.join(", "),
            )),
            Self::On | Self::Off | Self::Unset => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variables::Variables;

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
    fn a_blank_value_is_unset() {
        for blank in ["", " ", "\t", " \t\r\n "] {
            assert_eq!(Flag::parse(blank), Flag::Unset, "{blank:?} must be unset");
            assert_eq!(Flag::read(VARIABLE, Some(blank)), None);
        }
        assert_eq!(Flag::read(VARIABLE, None), None);
    }

    #[test]
    fn an_unrecognised_spelling_leaves_the_default_and_says_which_spellings_are_accepted() {
        for typo in ["ture", "maybe", "2", "enabled"] {
            let flag = Flag::parse(typo);
            assert_eq!(flag, Flag::Unrecognised(typo.to_string()));
            assert_eq!(flag.enabled(), None, "{typo:?} must leave the default");
            assert_eq!(Flag::read(VARIABLE, Some(typo)), None);
            let warning = flag
                .warning(VARIABLE)
                .unwrap_or_else(|| panic!("{typo:?} must be reported"));
            assert!(warning.contains(VARIABLE), "{warning}");
            assert!(warning.contains(&format!("{typo:?}")), "{warning}");
            assert!(warning.contains("default"), "{warning}");
            for spelling in ON.iter().chain(OFF.iter()) {
                assert!(warning.contains(spelling), "{warning} omits {spelling}");
            }
        }
    }

    #[test]
    fn only_an_unrecognised_spelling_is_reported() {
        for flag in [Flag::On, Flag::Off, Flag::Unset] {
            assert_eq!(flag.warning(VARIABLE), None, "{flag:?}");
        }
    }

    #[test]
    fn the_environment_falls_back_to_the_default_for_empty_and_unknown() {
        let mut variables = Variables::isolated(&[VARIABLE]);
        assert!(Flag::environment(VARIABLE, true));
        assert!(!Flag::environment(VARIABLE, false));

        for (value, default, expected) in [
            ("", true, true),
            ("", false, false),
            ("maybe", true, true),
            ("maybe", false, false),
            (" true ", true, true),
            ("0", true, false),
            ("off", true, false),
            ("false", true, false),
            ("1", false, true),
            ("yes", false, true),
            ("on", false, true),
        ] {
            variables.set(VARIABLE, value);
            assert_eq!(
                Flag::environment(VARIABLE, default),
                expected,
                "{value:?} with default {default}"
            );
        }
    }
}

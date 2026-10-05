//! The ChatGPT plans a codex login can be on, named the way ChatGPT names them.

const PLANS: &[(&str, &str)] = &[
    ("free", "ChatGPT Free"),
    ("plus", "ChatGPT Plus"),
    ("pro", "ChatGPT Pro"),
    ("team", "ChatGPT Team"),
    ("business", "ChatGPT Business"),
    ("enterprise", "ChatGPT Enterprise"),
    ("edu", "ChatGPT Edu"),
];

/// A plan as codex's id token spells it, when it is one ChatGPT sells.
pub fn label(plan: &str) -> Option<&'static str> {
    PLANS
        .iter()
        .find(|(known, _)| *known == plan)
        .map(|(_, label)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_plan_is_named_as_chatgpt_names_it() {
        for (plan, expected) in [
            ("plus", Some("ChatGPT Plus")),
            ("pro", Some("ChatGPT Pro")),
            ("team", Some("ChatGPT Team")),
            ("enterprise", Some("ChatGPT Enterprise")),
            ("Pro", None),
            ("", None),
            ("unlimited", None),
        ] {
            assert_eq!(label(plan), expected, "{plan:?}");
        }
    }
}

//! The Claude plans a sign-in can be on, named the way the CLI names them.

const PLANS: &[(&str, &str)] = &[
    ("max", "Claude Max"),
    ("pro", "Claude Pro"),
    ("team", "Claude Team"),
    ("enterprise", "Claude Enterprise"),
];

/// What the CLI calls `subscription`, a plan as Claude's token endpoint and `claude auth status`
/// both spell it, when it is a plan the CLI knows.
pub fn label(subscription: &str) -> Option<&'static str> {
    PLANS
        .iter()
        .find(|(plan, _)| *plan == subscription)
        .map(|(_, label)| *label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_plan_is_named_as_the_cli_names_it() {
        for (subscription, expected) in [
            ("max", Some("Claude Max")),
            ("pro", Some("Claude Pro")),
            ("team", Some("Claude Team")),
            ("enterprise", Some("Claude Enterprise")),
            ("free", None),
            ("", None),
            ("Team", None),
        ] {
            assert_eq!(label(subscription), expected, "{subscription:?}");
        }
    }
}

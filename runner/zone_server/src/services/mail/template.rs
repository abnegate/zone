//! The wording of each account message.

use abnegate_secret::sanitize;

/// One account message, addressed to `name` and carrying its `link`, one-time for the account
/// flows.
///
/// The name is whatever the person typed as their display name, so it is
/// sanitised before it reaches a mail client, as is the budget a notice names.
/// The link is not: redaction would eat the token that is its whole point.
///
/// A budget notice names the `budget` it is about, with what it has `spent` this month of its
/// `limit`, both already formatted as money.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template<'a> {
    Verification {
        name: &'a str,
        link: &'a str,
    },
    PasswordReset {
        name: &'a str,
        link: &'a str,
    },
    BudgetWarning {
        name: &'a str,
        budget: &'a str,
        spent: &'a str,
        limit: &'a str,
        link: &'a str,
    },
    BudgetExhausted {
        name: &'a str,
        budget: &'a str,
        spent: &'a str,
        limit: &'a str,
        link: &'a str,
        hard: bool,
    },
}

const BUDGET_FOOTER: &str = "Spend reports and budgets are part of the Zone Team plan.";

impl Template<'_> {
    pub fn subject(&self) -> &'static str {
        match self {
            Self::Verification { .. } => "Verify your email address",
            Self::PasswordReset { .. } => "Reset your password",
            Self::BudgetWarning { .. } => "An AI budget has reached its alert threshold",
            Self::BudgetExhausted { .. } => "An AI budget is used up",
        }
    }

    pub fn body(&self) -> String {
        let name = sanitize(self.name());
        match self {
            Self::Verification { link, .. } => format!(
                "Hello {name},\n\
                 \n\
                 Thank you for signing up for Zone!\n\
                 \n\
                 Please verify your email address by clicking the link below:\n\
                 \n\
                 {link}\n\
                 \n\
                 This link will expire in 24 hours.\n\
                 \n\
                 If you did not create an account, you can safely ignore this email.\n\
                 \n\
                 Best regards,\n\
                 The Zone Team\n"
            ),
            Self::PasswordReset { link, .. } => format!(
                "Hello {name},\n\
                 \n\
                 We received a request to reset your password for your Zone account.\n\
                 \n\
                 Click the link below to reset your password:\n\
                 \n\
                 {link}\n\
                 \n\
                 This link will expire in 1 hour.\n\
                 \n\
                 If you did not request a password reset, you can safely ignore this email.\n\
                 Your password will not be changed unless you click the link above and create a new password.\n\
                 \n\
                 Best regards,\n\
                 The Zone Team\n"
            ),
            Self::BudgetWarning {
                budget,
                spent,
                limit,
                link,
                ..
            } => {
                let budget = sanitize(budget);
                format!(
                    "Hello {name},\n\
                     \n\
                     The monthly AI budget for {budget} has reached its alert threshold: {spent} of its {limit} limit is spent this month.\n\
                     \n\
                     You can see what was spent, and change the budget, under Settings › Spend:\n\
                     \n\
                     {link}\n\
                     \n\
                     This is the only alert for this budget this month. Budgets reset on the 1st (UTC).\n\
                     \n\
                     {BUDGET_FOOTER}\n\
                     \n\
                     Best regards,\n\
                     The Zone Team\n"
                )
            }
            Self::BudgetExhausted {
                budget,
                spent,
                limit,
                link,
                hard,
                ..
            } => {
                let budget = sanitize(budget);
                let consequence = match hard {
                    true => {
                        "It is a hard limit, so the requests it covers through Zone's LiteLLM are stopped until it resets on the 1st (UTC) or an organization admin raises it. Coding agents and direct providers keep running."
                    }
                    false => {
                        "It is not a hard limit, so nothing has been stopped. It resets on the 1st (UTC)."
                    }
                };
                format!(
                    "Hello {name},\n\
                     \n\
                     The monthly AI budget for {budget} is used up: {spent} of its {limit} limit is spent this month.\n\
                     \n\
                     {consequence}\n\
                     \n\
                     You can see what was spent, and change the budget, under Settings › Spend:\n\
                     \n\
                     {link}\n\
                     \n\
                     {BUDGET_FOOTER}\n\
                     \n\
                     Best regards,\n\
                     The Zone Team\n"
                )
            }
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Verification { name, .. }
            | Self::PasswordReset { name, .. }
            | Self::BudgetWarning { name, .. }
            | Self::BudgetExhausted { name, .. } => name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str =
        "https://zone.test/verify-email?token=8Xk2Qm9Lp4Rw7Tz1Vb6Nh3Yj5Fd0Gs8Ac2Ee4Ii6Ko";

    fn lines(body: &str) -> Vec<&str> {
        body.lines().collect()
    }

    #[test]
    fn each_template_has_its_subject() {
        let verification = Template::Verification {
            name: "Sam",
            link: LINK,
        };
        let reset = Template::PasswordReset {
            name: "Sam",
            link: LINK,
        };

        assert_eq!(verification.subject(), "Verify your email address");
        assert_eq!(reset.subject(), "Reset your password");
    }

    #[test]
    fn verification_greets_the_person_and_carries_the_link_on_its_own_line() {
        let body = Template::Verification {
            name: "Sam",
            link: LINK,
        }
        .body();
        let lines = lines(&body);

        assert_eq!(lines.first(), Some(&"Hello Sam,"));
        assert!(lines.contains(&LINK), "{body}");
        assert!(lines.contains(&"This link will expire in 24 hours."));
        assert_eq!(lines.last(), Some(&"The Zone Team"));
    }

    #[test]
    fn a_password_reset_greets_the_person_and_carries_the_link_on_its_own_line() {
        let body = Template::PasswordReset {
            name: "Sam",
            link: LINK,
        }
        .body();
        let lines = lines(&body);

        assert_eq!(lines.first(), Some(&"Hello Sam,"));
        assert!(lines.contains(&LINK), "{body}");
        assert!(lines.contains(&"This link will expire in 1 hour."));
        assert_eq!(lines.last(), Some(&"The Zone Team"));
    }

    #[test]
    fn control_sequences_in_a_name_are_stripped_and_the_link_is_left_whole() {
        let name = "Sam\u{1b}[2J\u{1b}]8;;https://evil.test\u{7}\u{202e}\u{200d}";
        assert_ne!(
            sanitize(LINK),
            LINK,
            "the link must be one that sanitising would change"
        );

        for template in [
            Template::Verification { name, link: LINK },
            Template::PasswordReset { name, link: LINK },
        ] {
            let body = template.body();
            let lines = lines(&body);

            assert_eq!(lines.first(), Some(&"Hello Sam,"), "{body:?}");
            assert!(lines.contains(&LINK), "{body:?}");
        }
    }

    const SPEND: &str = "https://zone.test/settings/organization/spend";

    fn warning(name: &'static str, budget: &'static str) -> Template<'static> {
        Template::BudgetWarning {
            name,
            budget,
            spent: "$40.00",
            limit: "$50.00",
            link: SPEND,
        }
    }

    fn exhausted(hard: bool) -> Template<'static> {
        Template::BudgetExhausted {
            name: "Sam",
            budget: "the Design workspace in Acme",
            spent: "$50.10",
            limit: "$50.00",
            link: SPEND,
            hard,
        }
    }

    #[test]
    fn a_budget_warning_names_the_budget_what_is_spent_and_where_to_change_it() {
        let template = warning("Sam", "the Acme organization");
        let body = template.body();
        let lines = lines(&body);

        assert_eq!(
            template.subject(),
            "An AI budget has reached its alert threshold"
        );
        assert_eq!(lines.first(), Some(&"Hello Sam,"));
        assert!(
            lines.contains(
                &"The monthly AI budget for the Acme organization has reached its alert threshold: $40.00 of its $50.00 limit is spent this month."
            ),
            "{body}"
        );
        assert!(lines.contains(&SPEND), "{body}");
        assert!(lines.contains(&BUDGET_FOOTER), "{body}");
        assert_eq!(lines.last(), Some(&"The Zone Team"));
    }

    #[test]
    fn a_used_up_budget_says_whether_it_stopped_anything() {
        let hard = exhausted(true).body();
        let soft = exhausted(false).body();

        assert_eq!(exhausted(true).subject(), "An AI budget is used up");
        for body in [&hard, &soft] {
            assert!(
                lines(body).contains(
                    &"The monthly AI budget for the Design workspace in Acme is used up: $50.10 of its $50.00 limit is spent this month."
                ),
                "{body}"
            );
            assert!(lines(body).contains(&SPEND), "{body}");
        }
        assert!(
            hard.contains("are stopped until it resets on the 1st (UTC)"),
            "{hard}"
        );
        assert!(
            hard.contains("Coding agents and direct providers keep running."),
            "{hard}"
        );
        assert!(soft.contains("nothing has been stopped"), "{soft}");
        assert!(!soft.contains("are stopped"), "{soft}");
    }

    #[test]
    fn a_budget_notice_names_the_team_plan_and_never_enterprise() {
        for template in [
            warning("Sam", "the Acme organization"),
            exhausted(true),
            exhausted(false),
        ] {
            let text = format!("{}\n{}", template.subject(), template.body());

            assert!(text.contains("Zone Team plan"), "{text}");
            assert!(!text.to_lowercase().contains("enterprise"), "{text}");
        }
    }

    #[test]
    fn control_sequences_in_a_budget_notice_are_stripped_and_the_link_is_left_whole() {
        let body = warning(
            "Sam\u{1b}[2J\u{202e}",
            "the Design\u{1b}]8;;https://evil.test\u{7} workspace",
        )
        .body();

        assert_eq!(lines(&body).first(), Some(&"Hello Sam,"), "{body:?}");
        assert!(!body.contains('\u{1b}'), "{body:?}");
        assert!(!body.contains('\u{202e}'), "{body:?}");
        assert!(lines(&body).contains(&SPEND), "{body:?}");
    }
}

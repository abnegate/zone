//! The wording of each account message.

use abnegate_secret::sanitize;

/// One account message, addressed to `name` and carrying its one-time `link`.
///
/// The name is whatever the person typed as their display name, so it is
/// sanitised before it reaches a mail client. The link is not: redaction would
/// eat the token that is its whole point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template<'a> {
    Verification { name: &'a str, link: &'a str },
    PasswordReset { name: &'a str, link: &'a str },
}

impl Template<'_> {
    pub fn subject(&self) -> &'static str {
        match self {
            Self::Verification { .. } => "Verify your email address",
            Self::PasswordReset { .. } => "Reset your password",
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
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Verification { name, .. } | Self::PasswordReset { name, .. } => name,
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
}

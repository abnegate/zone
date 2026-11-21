use std::fmt;

use chrono::{DateTime, Utc};
use zone_enterprise::{GRACE_DAYS, Plan};

use crate::mail::escape;

const DATE_FORMAT: &str = "%-d %B %Y";

const OFFLINE: &str = "Zone verifies the key offline, so your server never needs to reach us.";

const RENEWAL: &str = "Each time your subscription renews you get a new key by email. \
                       Add it the same way to replace this one.";

const SIGN_OFF: &str = "The Zone Team";

/// The customer name arrives already normalised by the order, so the letter
/// shows exactly the name the license carries; it is only escaped for HTML.
#[derive(Clone, PartialEq, Eq)]
pub struct Letter {
    subject: String,
    text: String,
    html: String,
}

impl Letter {
    pub fn license(
        customer: &str,
        plan: Plan,
        seats: u32,
        expires_at: DateTime<Utc>,
        key: &str,
    ) -> Self {
        let date = expires_at.format(DATE_FORMAT).to_string();
        Self {
            subject: subject(plan),
            text: text(customer, plan, seats, &date, key),
            html: html(customer, plan, seats, &date, key),
        }
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn html(&self) -> &str {
        &self.html
    }
}

impl fmt::Debug for Letter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Letter")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

pub fn subject(plan: Plan) -> String {
    format!("Your Zone {} license key", name(plan))
}

fn name(plan: Plan) -> &'static str {
    match plan {
        Plan::Team => "Team",
        Plan::Enterprise => "Enterprise",
    }
}

fn thanks(plan: Plan) -> String {
    format!("Thank you for buying Zone {}.", name(plan))
}

fn grace() -> String {
    format!(
        "When the license expires, its features keep working for a \
         {GRACE_DAYS}-day grace period."
    )
}

fn text(customer: &str, plan: Plan, seats: u32, date: &str, key: &str) -> String {
    let thanks = thanks(plan);
    let name = name(plan);
    let grace = grace();
    format!(
        "Hello,\n\
         \n\
         {thanks} Here is your license key.\n\
         \n\
         Customer: {customer}\n\
         Plan: {name}\n\
         Seats: {seats}\n\
         Paid through: {date}\n\
         \n\
         {key}\n\
         \n\
         To add it:\n\
         \n\
         1. Sign in to Zone as a server admin.\n\
         2. Open Settings → This server → License.\n\
         3. Paste the key into License key.\n\
         4. Choose Add license, or Replace license if the server already has one.\n\
         \n\
         {OFFLINE} {grace}\n\
         \n\
         {RENEWAL}\n\
         \n\
         {SIGN_OFF}\n"
    )
}

fn html(customer: &str, plan: Plan, seats: u32, date: &str, key: &str) -> String {
    let customer = escape::html(customer);
    let date = escape::html(date);
    let key = escape::html(key);
    let subject = subject(plan);
    let thanks = thanks(plan);
    let name = name(plan);
    let grace = grace();
    format!(
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head><meta charset=\"utf-8\"><title>{subject}</title></head>\n\
         <body style=\"font-family: sans-serif; line-height: 1.5; color: #1a1a1a;\">\n\
         <p>Hello,</p>\n\
         <p>{thanks} Here is your license key.</p>\n\
         <table role=\"presentation\">\n\
         <tr><th align=\"left\">Customer</th><td>{customer}</td></tr>\n\
         <tr><th align=\"left\">Plan</th><td>{name}</td></tr>\n\
         <tr><th align=\"left\">Seats</th><td>{seats}</td></tr>\n\
         <tr><th align=\"left\">Paid through</th><td>{date}</td></tr>\n\
         </table>\n\
         <pre style=\"white-space: pre-wrap; word-break: break-all; padding: 12px; \
         background: #f4f4f5; border-radius: 6px;\">{key}</pre>\n\
         <p>To add it:</p>\n\
         <ol>\n\
         <li>Sign in to Zone as a server admin.</li>\n\
         <li>Open <strong>Settings → This server → License</strong>.</li>\n\
         <li>Paste the key into <strong>License key</strong>.</li>\n\
         <li>Choose <strong>Add license</strong>, or <strong>Replace license</strong> \
         if the server already has one.</li>\n\
         </ol>\n\
         <p>{OFFLINE} {grace}</p>\n\
         <p>{RENEWAL}</p>\n\
         <p>{SIGN_OFF}</p>\n\
         </body>\n\
         </html>\n"
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const KEY: &str = "eyJpZCI6IjAxOTMiLCJjdXN0b21lciI6IkFjbWUifQ.c2lnbmF0dXJlLWJ5dGVzLWhlcmUtXy0";

    fn expires_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2027, 11, 18, 11, 59, 58).unwrap()
    }

    fn letter(customer: &str) -> Letter {
        Letter::license(customer, Plan::Enterprise, 5, expires_at(), KEY)
    }

    fn team() -> Letter {
        Letter::license("Acme Ltd", Plan::Team, 5, expires_at(), KEY)
    }

    #[test]
    fn the_subject_names_the_plan() {
        assert_eq!(
            letter("Acme Ltd").subject(),
            "Your Zone Enterprise license key"
        );
        assert_eq!(team().subject(), "Your Zone Team license key");
        for plan in Plan::ALL {
            assert_eq!(
                Letter::license("Acme Ltd", plan, 5, expires_at(), KEY).subject(),
                subject(plan)
            );
        }
    }

    #[test]
    fn both_bodies_thank_the_customer_for_the_plan_they_bought() {
        for (letter, plan) in [(letter("Acme Ltd"), "Enterprise"), (team(), "Team")] {
            let thanks = format!("Thank you for buying Zone {plan}.");
            assert!(letter.text().contains(&thanks), "{}", letter.text());
            assert!(letter.html().contains(&thanks), "{}", letter.html());
            assert!(
                letter
                    .html()
                    .contains(&format!("<title>Your Zone {plan} license key</title>")),
                "{}",
                letter.html()
            );
        }
    }

    #[test]
    fn a_team_letter_never_mentions_enterprise() {
        let letter = team();
        assert!(!letter.text().contains("Enterprise"), "{}", letter.text());
        assert!(!letter.html().contains("Enterprise"), "{}", letter.html());
    }

    #[test]
    fn the_key_sits_alone_on_its_own_line_in_the_text() {
        let letter = letter("Acme Ltd");
        assert!(
            letter.text().lines().any(|line| line == KEY),
            "{}",
            letter.text()
        );
    }

    #[test]
    fn the_key_is_preformatted_and_intact_in_the_html() {
        let letter = letter("Acme Ltd");
        assert!(
            letter
                .html()
                .contains(&format!("border-radius: 6px;\">{KEY}</pre>")),
            "{}",
            letter.html()
        );
    }

    #[test]
    fn both_bodies_state_the_customer_plan_seats_and_paid_through_date() {
        let letter = letter("Acme Ltd");
        let text: Vec<&str> = letter.text().lines().collect();
        assert!(text.contains(&"Customer: Acme Ltd"));
        assert!(text.contains(&"Plan: Enterprise"));
        assert!(text.contains(&"Seats: 5"));
        assert!(text.contains(&"Paid through: 18 November 2027"));
        assert!(letter.html().contains("<td>Acme Ltd</td>"));
        assert!(letter.html().contains("<td>Enterprise</td>"));
        assert!(letter.html().contains("<td>5</td>"));
        assert!(letter.html().contains("<td>18 November 2027</td>"));
        assert!(team().text().lines().any(|line| line == "Plan: Team"));
        assert!(team().html().contains("<td>Team</td>"));
    }

    #[test]
    fn a_single_digit_day_is_not_padded() {
        let expires_at = Utc.with_ymd_and_hms(2027, 3, 4, 0, 0, 0).unwrap();
        let letter = Letter::license("Acme Ltd", Plan::Enterprise, 1, expires_at, KEY);
        assert!(letter.text().contains("Paid through: 4 March 2027\n"));
    }

    #[test]
    fn the_customer_name_is_escaped_in_the_html() {
        let letter = letter("Tom & Jerry's <script>\"Co\"</script>");
        assert!(
            letter.html().contains(
                "<td>Tom &amp; Jerry&#39;s &lt;script&gt;&quot;Co&quot;&lt;/script&gt;</td>"
            ),
            "{}",
            letter.html()
        );
        assert!(!letter.html().contains("<script>"));
        assert!(
            letter
                .text()
                .contains("Customer: Tom & Jerry's <script>\"Co\"</script>\n")
        );
    }

    #[test]
    fn both_bodies_walk_through_adding_the_key() {
        let letter = letter("Acme Ltd");
        for step in [
            "Settings → This server → License",
            "License key",
            "Add license",
            "Replace license",
            "server admin",
        ] {
            assert!(letter.text().contains(step), "text is missing {step}");
            assert!(letter.html().contains(step), "html is missing {step}");
        }
    }

    #[test]
    fn both_bodies_explain_offline_verification_and_the_grace_period() {
        for letter in [letter("Acme Ltd"), team()] {
            for body in [letter.text(), letter.html()] {
                assert!(body.contains("verifies the key offline"));
                assert!(body.contains(&format!(
                    "its features keep working for a {GRACE_DAYS}-day grace period"
                )));
            }
        }
    }

    #[test]
    fn debug_leaves_out_the_key() {
        let rendered = format!("{:?}", letter("Acme Ltd"));
        assert!(!rendered.contains(KEY), "{rendered}");
        assert!(rendered.contains(&subject(Plan::Enterprise)));
    }
}

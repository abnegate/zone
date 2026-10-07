//! The messages an account flow sends: sign-up verification and password reset.
//!
//! Zone owns the wording; delivery is `abnegate_notify`'s [`Mailer`] through
//! the relay the `SMTP_*` variables describe, or any other
//! [`Mail`](abnegate_notify::Mail) a test hands [`AccountMail::new`]. The email
//! notification channel reads the same relay through [`Config`].
//!
//! [`Mailer`]: abnegate_notify::Mailer

mod account;
mod config;
mod error;
mod sender_policy;
mod template;
mod variable;

pub use account::AccountMail;
pub use config::Config;
pub use config::DEFAULT_PORT;
pub use error::Error;
pub use sender_policy::SenderPolicy;
pub use template::Template;
pub use variable::Variable;

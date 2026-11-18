mod escape;
mod failure;
mod letter;
mod mail_error;
mod outbox;
mod relay;

pub use failure::Failure;
pub use letter::{Letter, SUBJECT};
pub use mail_error::MailError;
pub use outbox::Outbox;
pub use relay::{Relay, SEND_TIMEOUT};

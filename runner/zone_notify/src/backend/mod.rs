//! The channels this crate ships with.
//!
//! Each backend is one file implementing [`Notifier`](crate::Notifier).
//! Adding another is the same: a file here, exported below.

mod discord;
mod email;
mod slack;
mod smtp;
mod webhook;

pub use discord::Discord;
pub use email::Email;
pub use slack::Slack;
pub use smtp::SmtpConfig;

//! Chat history, context capacity, and the storage a chat session needs.
//!
//! [`store::ContextStore`] is the boundary: a session leases the right to
//! respond in one chat, writes the turn through that lease, and reads history
//! back, without naming a database. [`capacity`] and [`history`] are pure
//! functions over those types, so they can be tested without one.

pub mod capacity;
pub mod history;
pub mod store;

pub use history::{Entry, Evidence, History, NewEntry, ReplayMessage, Summary};
pub use store::{ContextStore, Error, Guard, Lease, StoredMessage, keep_alive};

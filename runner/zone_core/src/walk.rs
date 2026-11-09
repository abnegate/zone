//! How long Zone's file tools read a tree before answering.

use std::time::Duration;

/// How long a listing or a search reads, `rg` and any walk after it
/// together, before it answers with what it has found.
pub const BUDGET: Duration = Duration::from_secs(25);

//! One file per named block of the system prompt.

pub(in crate::agent::prompt) mod boundary;
pub(in crate::agent::prompt) mod cluster;
pub(in crate::agent::prompt) mod conduct;
pub(in crate::agent::prompt) mod files;
pub(in crate::agent::prompt) mod identity;
pub(in crate::agent::prompt) mod images;
pub(in crate::agent::prompt) mod mcp;
pub(in crate::agent::prompt) mod refusal;
pub(in crate::agent::prompt) mod reply;
pub(in crate::agent::prompt) mod retrieval;
pub(in crate::agent::prompt) mod session;
pub(in crate::agent::prompt) mod task;
pub(in crate::agent::prompt) mod tiers;
pub(in crate::agent::prompt) mod web;
pub(in crate::agent::prompt) mod workspace;

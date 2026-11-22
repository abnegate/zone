//! What SCIM provisioning stores per organization: the tokens identity providers authenticate
//! with, the users and groups they pushed, the roles those groups map to, and the recent
//! failures an owner reads in the console. Every query is scoped by organization.

pub mod accounts;
pub mod admission;
pub mod applied;
pub mod candidate;
pub mod criterion;
pub mod deactivation;
pub mod error_record;
pub mod errors;
pub mod group_member;
pub mod group_members;
pub mod group_record;
pub mod group_reference;
pub mod groups;
pub mod joining;
pub mod link;
pub mod listing;
pub mod mapping;
pub mod member;
pub mod membership;
pub mod new_error;
pub mod new_token;
pub mod outcome;
pub mod precedence;
pub mod removal;
pub mod roles;
pub mod roster_entry;
pub mod rosters;
pub mod token_credential;
pub mod token_record;
pub mod tokens;
pub mod trigger;
pub mod users;

#[cfg(test)]
mod tests;

pub use admission::Admission;
pub use applied::Applied;
pub use candidate::Candidate;
pub use criterion::Criterion;
pub use deactivation::Deactivation;
pub use error_record::ErrorRecord;
pub use group_member::GroupMember;
pub use group_record::GroupRecord;
pub use group_reference::GroupReference;
pub use joining::Joining;
pub use link::Link;
pub use listing::Listing;
pub use mapping::Mapping;
pub use member::Member;
pub use new_error::NewError;
pub use new_token::NewToken;
pub use outcome::Outcome;
pub use removal::Removal;
pub use roster_entry::RosterEntry;
pub use token_credential::TokenCredential;
pub use token_record::TokenRecord;
pub use trigger::Trigger;

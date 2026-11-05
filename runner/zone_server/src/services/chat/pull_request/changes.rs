//! The pull request changes Zone makes through abnegate-vcs: turning GitHub's
//! auto-merge on or off, and moving a pull request between draft and ready
//! for review. The crate release that carries those calls is pending, so each
//! is refused in words until it lands.

use abnegate_secret::SecretValue;
use abnegate_vcs::{PullRequestReference, PullRequestService};

use super::error::Error;

pub const PENDING_RELEASE: &str = "Coming with the next abnegate-vcs release.";

pub async fn set_auto_merge(
    _service: &PullRequestService,
    _reference: &PullRequestReference,
    _token: &SecretValue,
    _enabled: bool,
) -> Result<(), Error> {
    Err(Error::Refused(PENDING_RELEASE.to_string()))
}

pub async fn set_draft(
    _service: &PullRequestService,
    _reference: &PullRequestReference,
    _token: &SecretValue,
    _draft: bool,
) -> Result<(), Error> {
    Err(Error::Refused(PENDING_RELEASE.to_string()))
}

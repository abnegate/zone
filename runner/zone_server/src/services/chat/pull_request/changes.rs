//! The pull request changes Zone makes through abnegate-vcs: turning GitHub's
//! auto-merge on or off, moving a pull request between draft and ready for
//! review, and merging one.

use abnegate_vcs::AutoMerge as Request;

use super::error::Error;
use super::open::Open;

pub const NO_METHOD: &str = "The repository allows no merge method.";

/// The merge is asked for at the head the status was read at, so a branch
/// that moved since the reader last saw it is refused rather than merged
/// unseen. Branch protection is never bypassed: a rule that refuses the merge
/// is reported to the reader as GitHub stated it.
const ADMINISTRATOR: bool = false;

pub async fn set_auto_merge(open: &Open, enabled: bool) -> Result<(), Error> {
    if !enabled {
        return Ok(open
            .service
            .disable_auto_merge(&open.node_id, &open.token)
            .await?);
    }
    if let Some(reason) = &open.status.auto_merge.unavailable {
        return Err(Error::Refused(reason.clone()));
    }
    let method = open
        .methods
        .first()
        .copied()
        .ok_or_else(|| Error::Refused(NO_METHOD.to_string()))?;
    let request = Request::new(method).with_head(open.head.clone());
    Ok(open
        .service
        .enable_auto_merge(&open.node_id, &open.token, &request)
        .await?)
}

/// Merges the pull request now, by the first method its repository allows.
pub async fn merge(open: &Open) -> Result<(), Error> {
    if let Some(reason) = &open.status.merge_unavailable {
        return Err(Error::Refused(reason.clone()));
    }
    let method = open
        .methods
        .first()
        .copied()
        .ok_or_else(|| Error::Refused(NO_METHOD.to_string()))?;
    let title = format!("{} (#{})", open.status.title, open.reference.number());
    open.service
        .merge(
            &open.reference,
            &open.token,
            Some(&open.node_id),
            &open.head,
            &title,
            "",
            method,
            ADMINISTRATOR,
        )
        .await?;
    Ok(())
}

pub async fn set_draft(open: &Open, draft: bool) -> Result<(), Error> {
    let changed = if draft {
        open.service
            .convert_to_draft(&open.node_id, &open.token)
            .await
    } else {
        open.service
            .mark_ready_for_review(&open.node_id, &open.token)
            .await
    };
    Ok(changed?)
}

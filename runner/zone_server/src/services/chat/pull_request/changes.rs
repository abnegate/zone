//! The pull request changes Zone makes through abnegate-vcs: turning GitHub's
//! auto-merge on or off, and moving a pull request between draft and ready
//! for review.

use abnegate_vcs::AutoMerge as Request;

use super::error::Error;
use super::open::Open;

pub const NO_METHOD: &str = "The repository allows no merge method.";

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

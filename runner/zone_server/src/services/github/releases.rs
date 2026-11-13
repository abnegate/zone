mod canonical;
mod catalogue;
mod created;
mod failure;
mod included_pull;
mod listing;
mod notes;
mod offered;
mod preview;
mod proposal;
mod reading;
mod recent;
mod reconciliation;
mod refusal;
mod release;
mod source_options;
mod tag;
#[cfg(test)]
mod tests;

pub use created::Created;
pub(crate) use failure::Failure;
pub use included_pull::IncludedPull;
pub use listing::Listing;
pub(crate) use offered::Offered;
pub use preview::Preview;
pub(crate) use proposal::Proposal;
pub(crate) use recent::Recent;
pub(crate) use reconciliation::Reconciliation;
pub(crate) use refusal::Refusal;
pub use release::Release;
pub use source_options::SourceOptions;

use abnegate_vcs::CommitSha;
use abnegate_vcs::MakeLatest;
use abnegate_vcs::ObjectId;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::Release as Published;
use abnegate_vcs::ReleaseRequest;
use chrono::TimeDelta;
use chrono::Utc;
use futures::future::join_all;
use uuid::Uuid;

use super::authentication::authenticated;
use super::client::Github;
use super::client::described;
use super::connection::ConnectedSource;
use super::connection::connect;
use super::connection::connected_sources;
use super::release::ReleasePipeline;
use super::unconnected::Unconnected;
use crate::db::sources::SourceRow;
use crate::state::AppState;
use canonical::Canonical;
use catalogue::Catalogue;
use failure::INVALID_TAG;
use notes::Notes;
use tag::Tag;

const NOT_GITHUB: &str = "Not a GitHub source.";
const MARKER: &str = "zone-release";

pub(crate) async fn recent(
    state: &AppState,
    workspace_id: Uuid,
    days: Option<u32>,
    refresh: bool,
) -> Result<Recent, Failure> {
    let window_days = state.config().github.release_window(days);
    let listed = connected_sources(state, workspace_id).await?;
    let now = Utc::now();
    let origin = state.config().github_web_url.as_str();
    let reads = join_all(listed.connected.iter().map(|source| {
        state.github_snapshots().releases.read(
            (source.source.id, window_days),
            refresh,
            now,
            move |previous| async move {
                let since = now - TimeDelta::days(i64::from(window_days));
                let checked_at = now.to_rfc3339();
                let previous = previous.as_deref();
                authenticated(
                    state,
                    &source.source,
                    source.configuration.clone(),
                    |github| {
                        let checked_at = checked_at.as_str();
                        async move {
                            github
                                .recent_listing(source, origin, since, previous, checked_at)
                                .await
                        }
                    },
                )
                .await
            },
        )
    }))
    .await;
    let mut recent = Recent::new(window_days, now, unusable(&listed.invalid));
    for (source, read) in listed.connected.iter().zip(reads) {
        match read {
            Ok((snapshot, freshness)) => recent.read(source, origin, &snapshot, freshness),
            Err(warning) => recent.failed(source, origin, warning),
        }
    }
    Ok(recent.sorted())
}

pub(crate) async fn pipeline(
    state: &AppState,
    workspace_id: Uuid,
    source_id: Uuid,
    release_id: u64,
    refresh: bool,
) -> Result<ReleasePipeline, Failure> {
    let source = &connect(state, workspace_id, source_id).await?;
    let now = Utc::now();
    let (snapshot, _) = state
        .github_snapshots()
        .pipelines
        .read(
            (source_id, release_id),
            refresh,
            now,
            move |previous| async move {
                let checked_at = now.to_rfc3339();
                let previous = previous.as_deref();
                authenticated(
                    state,
                    &source.source,
                    source.configuration.clone(),
                    |github| {
                        let checked_at = checked_at.as_str();
                        async move {
                            github
                                .observed_pipeline(release_id, previous, checked_at)
                                .await
                        }
                    },
                )
                .await
            },
        )
        .await
        .map_err(Failure::read)?;
    Ok(ReleasePipeline::clone(&snapshot.value))
}

pub(crate) async fn options(
    state: &AppState,
    workspace_id: Uuid,
    refresh: bool,
) -> Result<Offered, Failure> {
    let listed = connected_sources(state, workspace_id).await?;
    let now = Utc::now();
    let origin = state.config().github_web_url.as_str();
    let reads = join_all(listed.connected.iter().map(|source| {
        state
            .github_snapshots()
            .options
            .read(source.source.id, refresh, now, move |_| {
                authenticated(
                    state,
                    &source.source,
                    source.configuration.clone(),
                    move |github| async move {
                        github
                            .source_options(source, origin)
                            .await
                            .map_err(described)
                    },
                )
            })
    }))
    .await;
    let mut offered = Offered::new(now, unusable(&listed.invalid));
    for (source, read) in listed.connected.iter().zip(reads) {
        match read {
            Ok((snapshot, _)) => offered.read(source, &snapshot),
            Err(warning) => offered.failed(source, &warning),
        }
    }
    Ok(offered)
}

pub(crate) async fn preview(
    state: &AppState,
    workspace_id: Uuid,
    proposal: &Proposal,
) -> Result<Preview, Failure> {
    let tag = &Tag::parse(&proposal.tag).ok_or(Failure::Invalid(INVALID_TAG))?;
    let source = &connect(state, workspace_id, proposal.source_id).await?;
    let origin = state.config().github_web_url.as_str();
    attempt(state, source, |github| async move {
        propose(&github, source, origin, tag, proposal).await
    })
    .await
}

pub(crate) async fn create(
    state: &AppState,
    workspace_id: Uuid,
    proposal: &Proposal,
    prerelease: bool,
    digest: &str,
) -> Result<Created, Failure> {
    let tag = &Tag::parse(&proposal.tag).ok_or(Failure::Invalid(INVALID_TAG))?;
    let source = &connect(state, workspace_id, proposal.source_id).await?;
    let origin = state.config().github_web_url.as_str();
    let snapshots = state.github_snapshots();
    let _releasing = snapshots
        .try_release(proposal.source_id)
        .ok_or(Failure::Refused(Refusal::InProgress))?;
    let marker = Uuid::new_v4();
    let created = attempt(state, source, |github| async move {
        let preview = propose(&github, source, origin, tag, proposal).await?;
        if preview.digest != digest {
            return Err(Refusal::PreviewStale.into());
        }
        let published = publish(&github, &preview, prerelease, marker).await?;
        Ok(Created {
            source_id: source.source.id,
            id: published.id,
            tag: published.tag,
            name: published.name,
            url: published.url,
            published_at: published.published_at.unwrap_or_default(),
        })
    })
    .await;
    snapshots.invalidate(proposal.source_id);
    created
}

async fn attempt<T, Operation, Attempted>(
    state: &AppState,
    source: &ConnectedSource,
    operation: Operation,
) -> Result<T, Failure>
where
    Operation: Fn(Github) -> Attempted,
    Attempted: Future<Output = Result<T, Failure>>,
{
    authenticated(
        state,
        &source.source,
        source.configuration.clone(),
        |github| {
            let attempted = operation(github);
            async move {
                match attempted.await {
                    Err(Failure::Unauthenticated(message)) => Err(message),
                    outcome => Ok(outcome),
                }
            }
        },
    )
    .await
    .map_err(Failure::Upstream)?
}

async fn propose(
    github: &Github,
    source: &ConnectedSource,
    origin: &str,
    tag: &Tag,
    proposal: &Proposal,
) -> Result<Preview, Failure> {
    let names = github.tag_names().await?;
    let latest = Catalogue::new(names.iter().map(String::as_str)).latest_tag();
    if latest != proposal.expected_latest_tag {
        return Err(Refusal::LatestTagMoved {
            expected: proposal.expected_latest_tag.clone(),
            latest,
        }
        .into());
    }
    if github.tag_held(tag.name()).await? {
        return Err(Refusal::TagExists {
            tag: tag.name().to_string(),
            tag_object: None,
        }
        .into());
    }
    let (_, target) = github.branch_head().await?;
    let notes = github
        .release_notes(tag.name(), &target, latest.as_deref())
        .await?;
    let repository = source.configuration.full_name();
    let pulls = Notes::new(&notes.body).pulls(&source.configuration.web_url(origin));
    let digest = Canonical::new(
        &repository,
        tag.name(),
        &notes.name,
        &notes.body,
        latest.as_deref(),
        target.as_str(),
        &pulls,
    )
    .digest();
    Ok(Preview {
        source_id: source.source.id,
        repository,
        tag: tag.name().to_string(),
        name: notes.name,
        body: notes.body,
        base_tag: latest,
        target: target.to_string(),
        pulls,
        digest,
    })
}

async fn publish(
    github: &Github,
    preview: &Preview,
    prerelease: bool,
    marker: Uuid,
) -> Result<Published, Failure> {
    let target = CommitSha::parse(&preview.target)
        .map_err(|error| Failure::from(PullRequestError::Parse(error)))?;
    let marked = format!("<!-- {MARKER}:{marker} -->");
    let object = github
        .write_tag(
            &preview.tag,
            &format!("{}\n\n{marked}", preview.name),
            target.clone(),
        )
        .await?;
    let reconcile = |reference_created: bool, release_id: Option<u64>| {
        Failure::Refused(Refusal::ReconciliationRequired(Reconciliation {
            tag: preview.tag.clone(),
            tag_object: object.to_string(),
            reference_created,
            release_id,
        }))
    };
    reference(github, &preview.tag, &object)
        .await
        .map_err(|refused| refused.unwrap_or_else(|| reconcile(false, None)))?;
    let request = ReleaseRequest::new(preview.tag.as_str())
        .with_target(target.as_str())
        .with_name(preview.name.as_str())
        .with_body(format!("{}\n\n{marked}", preview.body))
        .with_draft(true)
        .with_prerelease(prerelease)
        .with_make_latest(if prerelease {
            MakeLatest::False
        } else {
            MakeLatest::True
        });
    let draft = github
        .write_draft_release(&request)
        .await
        .map_err(|_| reconcile(true, None))?;
    github
        .publish_draft(draft.id)
        .await
        .map_err(|_| reconcile(true, Some(draft.id)))?;
    match github.release_by_id(draft.id).await {
        Ok(published)
            if !published.draft
                && published.tag == preview.tag
                && published.published_at.is_some() =>
        {
            Ok(published)
        }
        _ => Err(reconcile(true, Some(draft.id))),
    }
}

async fn reference(github: &Github, tag: &str, object: &ObjectId) -> Result<(), Option<Failure>> {
    let existed = match github.write_reference(tag, object).await {
        Ok(()) => return Ok(()),
        Err(error) => matches!(error, PullRequestError::ReferenceExists(_)),
    };
    let taken = || {
        Some(Failure::from(Refusal::TagExists {
            tag: tag.to_string(),
            tag_object: Some(object.to_string()),
        }))
    };
    match github.tag_reference(tag).await {
        Ok(Some(reference)) if reference.object == *object => Ok(()),
        Ok(Some(_)) => Err(taken()),
        _ if existed => Err(taken()),
        Ok(None) | Err(_) => Err(None),
    }
}

fn unusable(invalid: &[SourceRow]) -> Vec<String> {
    invalid
        .iter()
        .map(|source| {
            format!(
                "{}: {}",
                source.name,
                Unconnected::InvalidConfiguration.message(NOT_GITHUB)
            )
        })
        .collect()
}

mod canonical;
mod catalogue;
mod created;
mod failure;
mod included_pull;
mod line;
mod listing;
mod notes;
mod offered;
mod placement;
mod preview;
mod proposal;
mod reading;
mod recent;
mod reconciliation;
mod refusal;
mod release;
mod source_options;
mod tag;
mod tag_line;
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
pub use tag_line::TagLine;

use std::sync::Arc;

use abnegate_vcs::CommitSha;
use abnegate_vcs::MakeLatest;
use abnegate_vcs::ObjectId;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::Release as Published;
use abnegate_vcs::ReleaseRequest;
use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use futures::future::join_all;
use uuid::Uuid;

use super::authentication::credentialed;
use super::client::Github;
use super::connection::ConnectedSource;
use super::connection::connect;
use super::connection::connected_sources;
use super::fault::Fault;
use super::freshness::Freshness;
use super::release::ReleasePipeline;
use super::snapshot::Snapshot;
use super::throttle::admitted;
use super::unconnected::NOT_GITHUB;
use super::unconnected::Unconnected;
use crate::config::MAXIMUM_RELEASE_WINDOW_DAYS;
use crate::db::sources::SourceRow;
use crate::state::AppState;
use canonical::Canonical;
use catalogue::Catalogue;
use failure::INVALID_TAG;
use failure::PRERELEASE_MISMATCH;
use failure::RELEASE_MISSING;
use notes::Notes;
use placement::Placement;
use tag::Tag;

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
    let reads = join_all(
        listed
            .connected
            .iter()
            .map(|source| listing(state, source, refresh, now)),
    )
    .await;
    let since = now - TimeDelta::days(i64::from(window_days));
    let mut recent = Recent::new(window_days, now, unusable(&listed.invalid));
    for (source, read) in listed.connected.iter().zip(reads) {
        match read {
            Ok((snapshot, freshness)) => {
                recent.read(source, origin, &windowed(&snapshot, since), freshness)
            }
            Err(fault) => recent.failed(source, origin, fault.to_string()),
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
    let (listed, _) = listing(state, source, false, now).await?;
    if !listed
        .value
        .releases
        .iter()
        .any(|release| release.id == release_id)
    {
        return Err(Failure::Missing(RELEASE_MISSING.to_string()));
    }
    let snapshots = state.github_snapshots();
    let permits = snapshots.throttle.credential(&source.source);
    let (snapshot, _) = snapshots
        .pipelines
        .read(
            (source_id, release_id),
            refresh,
            now,
            move |previous| async move {
                let _permit = admitted(&permits).await?;
                let checked_at = now.to_rfc3339();
                let previous = previous.as_deref();
                credentialed(
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
        .await?;
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
    let snapshots = state.github_snapshots();
    let reads = join_all(listed.connected.iter().map(|source| {
        let permits = snapshots.throttle.credential(&source.source);
        snapshots
            .options
            .read(source.source.id, refresh, now, move |_| async move {
                let _permit = admitted(&permits).await?;
                credentialed(
                    state,
                    &source.source,
                    source.configuration.clone(),
                    move |github| async move { github.source_options(source, origin).await },
                )
                .await
            })
    }))
    .await;
    let mut offered = Offered::new(now, unusable(&listed.invalid));
    for (source, read) in listed.connected.iter().zip(reads) {
        match read {
            Ok((snapshot, _)) => offered.read(source, &snapshot),
            Err(fault) => offered.failed(source, &fault.to_string()),
        }
    }
    Ok(offered)
}

pub(crate) async fn preview(
    state: &AppState,
    workspace_id: Uuid,
    proposal: &Proposal,
) -> Result<Preview, Failure> {
    let tag = &Tag::parse(&proposal.tag).ok_or_else(|| Failure::invalid(INVALID_TAG))?;
    let source = &connect(state, workspace_id, proposal.source_id).await?;
    let origin = state.config().github_web_url.as_str();
    credentialed(
        state,
        &source.source,
        source.configuration.clone(),
        |github| async move {
            let (preview, _) = propose(&github, source, origin, tag, proposal).await?;
            Ok(preview)
        },
    )
    .await
}

pub(crate) async fn create(
    state: &AppState,
    workspace_id: Uuid,
    proposal: &Proposal,
    prerelease: bool,
    digest: &str,
) -> Result<Created, Failure> {
    let tag = &Tag::parse(&proposal.tag).ok_or_else(|| Failure::invalid(INVALID_TAG))?;
    if prerelease == tag.stable() {
        return Err(Failure::invalid(PRERELEASE_MISMATCH));
    }
    let source = &connect(state, workspace_id, proposal.source_id).await?;
    let origin = state.config().github_web_url.as_str();
    let snapshots = state.github_snapshots();
    let _releasing = snapshots
        .try_release(proposal.source_id)
        .ok_or(Failure::Refused(Refusal::InProgress))?;
    let marker = Uuid::new_v4();
    let created = credentialed(
        state,
        &source.source,
        source.configuration.clone(),
        |github| async move {
            let (preview, latest) = propose(&github, source, origin, tag, proposal).await?;
            if preview.digest != digest {
                return Err(Refusal::PreviewStale.into());
            }
            let published = publish(&github, &preview, latest, marker).await?;
            Ok(Created {
                source_id: source.source.id,
                id: published.id,
                tag: published.tag,
                name: published.name,
                url: published.url,
                published_at: published.published_at.unwrap_or_default(),
            })
        },
    )
    .await;
    snapshots.invalidate(proposal.source_id);
    created
}

async fn listing(
    state: &AppState,
    source: &ConnectedSource,
    refresh: bool,
    now: DateTime<Utc>,
) -> Result<(Snapshot<Listing>, Freshness), Fault> {
    let snapshots = state.github_snapshots();
    let permits = snapshots.throttle.credential(&source.source);
    let origin = state.config().github_web_url.as_str();
    snapshots
        .releases
        .read(source.source.id, refresh, now, move |previous| async move {
            let _permit = admitted(&permits).await?;
            let since = now - TimeDelta::days(MAXIMUM_RELEASE_WINDOW_DAYS as i64);
            let checked_at = now.to_rfc3339();
            let previous: Option<Arc<Listing>> = previous;
            let previous = previous.as_deref();
            credentialed(
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
        })
        .await
}

async fn propose(
    github: &Github,
    source: &ConnectedSource,
    origin: &str,
    tag: &Tag,
    proposal: &Proposal,
) -> Result<(Preview, MakeLatest), Failure> {
    let names = github.tag_names().await?;
    let catalogue = Catalogue::new(names.iter().map(String::as_str));
    let latest = catalogue.latest(tag.prefix());
    let latest_name = latest.map(|latest| latest.name().to_string());
    if latest_name != proposal.expected_latest_tag {
        return Err(Refusal::LatestTagMoved {
            expected: proposal.expected_latest_tag.clone(),
            latest: latest_name,
        }
        .into());
    }
    if let Some(latest) = latest.filter(|latest| !tag.newer_than(latest)) {
        return Err(Refusal::TagNotNewer {
            tag: tag.name().to_string(),
            latest: latest.name().to_string(),
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
    if let Some(release_id) = github.release_tagged(tag.name()).await? {
        return Err(Refusal::ReleaseExists {
            tag: tag.name().to_string(),
            release_id,
        }
        .into());
    }
    let make_latest = match catalogue.latest_stable() {
        _ if !tag.stable() => MakeLatest::False,
        Some(overall) if !tag.newer_than(overall) => MakeLatest::False,
        _ => MakeLatest::True,
    };
    let (_, target) = github.branch_head().await?;
    let notes = github
        .release_notes(tag.name(), &target, latest_name.as_deref())
        .await?;
    let repository = source.configuration.full_name();
    let pulls = Notes::new(&notes.body).pulls(&source.configuration.web_url(origin));
    let digest = Canonical::new(
        &repository,
        tag.name(),
        &notes.name,
        &notes.body,
        latest_name.as_deref(),
        target.as_str(),
        &pulls,
    )
    .digest();
    let preview = Preview {
        source_id: source.source.id,
        repository,
        tag: tag.name().to_string(),
        name: notes.name,
        body: notes.body,
        base_tag: latest_name,
        target: target.to_string(),
        prerelease: !tag.stable(),
        pulls,
        digest,
    };
    Ok((preview, make_latest))
}

async fn publish(
    github: &Github,
    preview: &Preview,
    latest: MakeLatest,
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
    let reconcile = |reference_created: Option<bool>, release_id: Option<u64>, exists: bool| {
        Failure::Refused(Refusal::ReconciliationRequired(Reconciliation {
            tag: preview.tag.clone(),
            tag_object: object.to_string(),
            reference_created,
            release_id,
            release_exists: exists,
            marker,
        }))
    };
    match placed(github, &preview.tag, &object).await {
        Placement::Ours => {}
        Placement::Taken => {
            return Err(Refusal::TagExists {
                tag: preview.tag.clone(),
                tag_object: Some(object.to_string()),
            }
            .into());
        }
        Placement::Absent => return Err(reconcile(Some(false), None, false)),
        Placement::Unknown => return Err(reconcile(None, None, false)),
    }
    let request = ReleaseRequest::new(preview.tag.as_str())
        .with_target(target.as_str())
        .with_name(preview.name.as_str())
        .with_body(format!("{}\n\n{marked}", preview.body))
        .with_draft(true)
        .with_prerelease(preview.prerelease)
        .with_make_latest(latest);
    let draft = match github.write_draft_release(&request).await {
        Ok(draft) => draft,
        Err(PullRequestError::ReleaseExists(_)) => return Err(reconcile(Some(true), None, true)),
        Err(_) => return Err(reconcile(Some(true), None, false)),
    };
    if github.publish_draft(draft.id).await.is_err() {
        return match github.release_by_id(draft.id).await {
            Ok(published) if live(&published, preview) => Ok(published),
            _ => Err(reconcile(Some(true), Some(draft.id), false)),
        };
    }
    match github.release_by_id(draft.id).await {
        Ok(published) if live(&published, preview) => Ok(published),
        _ => Err(reconcile(Some(true), Some(draft.id), false)),
    }
}

async fn placed(github: &Github, tag: &str, object: &ObjectId) -> Placement {
    if github.write_reference(tag, object).await.is_ok() {
        return Placement::Ours;
    }
    match github.tag_reference(tag).await {
        Ok(Some(found)) if found.object == *object => Placement::Ours,
        Ok(Some(_)) => Placement::Taken,
        Ok(None) => Placement::Absent,
        Err(_) => Placement::Unknown,
    }
}

fn live(published: &Published, preview: &Preview) -> bool {
    !published.draft && published.tag == preview.tag && published.published_at.is_some()
}

fn windowed(snapshot: &Snapshot<Listing>, since: DateTime<Utc>) -> Snapshot<Listing> {
    Snapshot {
        value: Arc::new(snapshot.value.since(since)),
        generated_at: snapshot.generated_at,
        warning: snapshot.warning.clone(),
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

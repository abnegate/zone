use std::collections::HashMap;
use std::num::NonZeroU32;

use abnegate_vcs::BranchName;
use abnegate_vcs::CommitSha;
use abnegate_vcs::GeneratedNotes;
use abnegate_vcs::GitReference;
use abnegate_vcs::NotesRequest;
use abnegate_vcs::ObjectId;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::Release as Published;
use abnegate_vcs::ReleaseRequest;
use abnegate_vcs::TagRequest;
use abnegate_vcs::Tagger;
use chrono::DateTime;
use chrono::Utc;
use reqwest::Url;

use super::Listing;
use super::Release;
use super::SourceOptions;
use super::catalogue::Catalogue;
use super::failure::RELEASE_MISSING;
use super::notes::Notes;
use crate::services::github::client::Github;
use crate::services::github::client::PER_PAGE;
use crate::services::github::client::described;
use crate::services::github::connection::ConnectedSource;
use crate::services::github::release;
use crate::services::github::release::Lookup;
use crate::services::github::release::ReleaseIdentity;
use crate::services::github::release::ReleasePipeline;

const RELEASE_PAGES: u32 = 10;
const TAG_PAGES: u32 = 3;
const TAGGER_NAME: &str = "Zone";
const TAGGER_EMAIL: &str = "zone@users.noreply.github.com";
const SCHEMES: [&str; 2] = ["http", "https"];

impl Github {
    pub(crate) async fn releases_since(
        &self,
        since: DateTime<Utc>,
    ) -> Result<(Vec<Published>, bool), PullRequestError> {
        let mut releases = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let listed = self
                .service()
                .list_releases(self.repository(), self.token(), page, PER_PAGE)
                .await?;
            let mut passed = false;
            for listed in listed.items.into_iter().filter(|listed| !listed.draft) {
                match listed.published_at.as_deref().and_then(moment) {
                    Some(published) if published >= since => releases.push(listed),
                    Some(_) => passed = true,
                    None => {}
                }
            }
            match listed.next {
                Some(next) if !passed && next.get() <= RELEASE_PAGES => page = next,
                Some(_) if !passed => return Ok((releases, false)),
                _ => return Ok((releases, true)),
            }
        }
    }

    pub(crate) async fn release_pipelines_for(
        &self,
        releases: &[ReleaseIdentity],
        previous: &HashMap<String, ReleasePipeline>,
        checked_at: &str,
    ) -> Vec<ReleasePipeline> {
        let sweep = self.pipeline_sweep(releases).await;
        let mut pipelines = Vec::with_capacity(releases.len());
        for identity in releases {
            let observed = match &sweep {
                Some(records) => ReleasePipeline::observe(identity.clone(), records, checked_at),
                None => ReleasePipeline::unavailable(identity.clone(), checked_at),
            };
            let observed = match observed.lookup {
                Lookup::Complete => observed,
                _ => release::merge(
                    Some(observed),
                    self.exact_pipeline(identity, checked_at).await,
                ),
            };
            pipelines.push(release::merge(
                previous.get(&identity.key()).cloned(),
                observed,
            ));
        }
        pipelines
    }

    pub(crate) async fn recent_listing(
        &self,
        source: &ConnectedSource,
        origin: &str,
        since: DateTime<Utc>,
        previous: Option<&Listing>,
        checked_at: &str,
    ) -> Result<Listing, String> {
        let (published, complete) = self.releases_since(since).await.map_err(described)?;
        let mut warnings = Vec::new();
        let mut usable = Vec::with_capacity(published.len());
        for published in published {
            match self.release_identity(&published) {
                Ok(identity) if linked(&published.url) => usable.push((published, identity)),
                Ok(_) => warnings.push(format!("Release {} has no usable link.", published.tag)),
                Err(error) => warnings.push(format!("Release {}: {error}", published.tag)),
            }
        }
        let identities: Vec<ReleaseIdentity> = usable
            .iter()
            .map(|(_, identity)| identity.clone())
            .collect();
        let earlier: HashMap<String, ReleasePipeline> = previous
            .into_iter()
            .flat_map(|listing| &listing.releases)
            .map(|release| (release.pipeline.release.key(), release.pipeline.clone()))
            .collect();
        let pipelines = self
            .release_pipelines_for(&identities, &earlier, checked_at)
            .await;
        let repository_url = source.configuration.web_url(origin);
        let releases = usable
            .into_iter()
            .zip(pipelines)
            .map(|((published, _), pipeline)| {
                let pulls = Notes::new(published.body.as_deref().unwrap_or_default())
                    .pulls(&repository_url);
                Release::observed(source, origin, &published, pulls, pipeline)
            })
            .collect();
        Ok(Listing {
            releases,
            complete,
            warnings,
        })
    }

    pub(crate) async fn observed_pipeline(
        &self,
        id: u64,
        previous: Option<&ReleasePipeline>,
        checked_at: &str,
    ) -> Result<ReleasePipeline, String> {
        let published = match self.release_by_id(id).await {
            Ok(published) if !published.draft && published.published_at.is_some() => published,
            Ok(_) | Err(PullRequestError::NotFound) => return Err(RELEASE_MISSING.to_string()),
            Err(error) => return Err(described(error)),
        };
        let identity = self.release_identity(&published)?;
        let observed = self.exact_pipeline(&identity, checked_at).await;
        let previous = previous
            .filter(|previous| previous.release.key() == identity.key())
            .cloned();
        Ok(release::merge(previous, observed))
    }

    pub(crate) async fn source_options(
        &self,
        source: &ConnectedSource,
        origin: &str,
    ) -> Result<SourceOptions, PullRequestError> {
        let (names, branch) = tokio::join!(
            self.tag_names(),
            self.service()
                .get_default_branch(self.repository(), self.token())
        );
        let catalogue = Catalogue::new(names?.iter().map(String::as_str));
        Ok(SourceOptions {
            source_id: source.source.id,
            repository: source.configuration.full_name(),
            repository_url: source.configuration.web_url(origin),
            default_branch: branch?.to_string(),
            latest_tag: catalogue.latest_tag(),
            next_tag: catalogue.next_tag(),
            previous_tags: catalogue.previous_tags(),
        })
    }

    pub(crate) async fn tag_names(&self) -> Result<Vec<String>, PullRequestError> {
        let mut names = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let listed = self
                .service()
                .list_tags(self.repository(), self.token(), page, PER_PAGE)
                .await?;
            names.extend(listed.items.into_iter().map(|tag| tag.tag));
            match listed.next {
                Some(next) if next.get() <= TAG_PAGES => page = next,
                _ => return Ok(names),
            }
        }
    }

    pub(crate) async fn tag_held(&self, tag: &str) -> Result<bool, PullRequestError> {
        self.service()
            .tag_exists(self.repository(), self.token(), tag)
            .await
    }

    pub(crate) async fn tag_reference(
        &self,
        tag: &str,
    ) -> Result<Option<GitReference>, PullRequestError> {
        self.service()
            .fetch_tag_reference(self.repository(), self.token(), tag)
            .await
    }

    pub(crate) async fn branch_head(&self) -> Result<(BranchName, CommitSha), PullRequestError> {
        let branch = self
            .service()
            .get_default_branch(self.repository(), self.token())
            .await?;
        let head = self
            .service()
            .resolve_commit(self.repository(), self.token(), branch.as_str())
            .await?;
        Ok((branch, head))
    }

    pub(crate) async fn release_notes(
        &self,
        tag: &str,
        target: &CommitSha,
        previous: Option<&str>,
    ) -> Result<GeneratedNotes, PullRequestError> {
        let request = NotesRequest::new(tag).with_target(target.as_str());
        let request = match previous {
            Some(previous) => request.with_previous_tag(previous),
            None => request,
        };
        self.service()
            .generate_release_notes(self.repository(), self.token(), &request)
            .await
    }

    pub(crate) async fn write_tag(
        &self,
        tag: &str,
        message: &str,
        commit: CommitSha,
    ) -> Result<ObjectId, PullRequestError> {
        let request = TagRequest::new(tag, message, commit, Tagger::new(TAGGER_NAME, TAGGER_EMAIL));
        self.service()
            .create_tag(self.repository(), self.token(), &request)
            .await
    }

    pub(crate) async fn write_reference(
        &self,
        tag: &str,
        object: &ObjectId,
    ) -> Result<(), PullRequestError> {
        self.service()
            .create_tag_reference(self.repository(), self.token(), tag, object)
            .await
    }

    pub(crate) async fn write_draft_release(
        &self,
        request: &ReleaseRequest,
    ) -> Result<Published, PullRequestError> {
        self.service()
            .create_release(self.repository(), self.token(), request)
            .await
    }

    pub(crate) async fn publish_draft(&self, id: u64) -> Result<Published, PullRequestError> {
        self.service()
            .publish_release(self.repository(), self.token(), id)
            .await
    }

    pub(crate) async fn release_by_id(&self, id: u64) -> Result<Published, PullRequestError> {
        self.service()
            .fetch_release_by_id(self.repository(), self.token(), id)
            .await
    }
}

fn moment(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|moment| moment.with_timezone(&Utc))
}

fn linked(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| SCHEMES.contains(&url.scheme()) && url.has_host())
}

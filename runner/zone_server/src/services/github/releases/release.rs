use serde::Serialize;
use uuid::Uuid;

use super::IncludedPull;
use crate::services::github::connection::ConnectedSource;
use crate::services::github::release::PipelineBlockerReport;
use crate::services::github::release::PipelineState;
use crate::services::github::release::ReleasePipeline;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    pub source_id: Uuid,
    pub repository: String,
    pub repository_url: String,
    pub id: u64,
    pub tag: String,
    pub name: Option<String>,
    pub url: String,
    pub published_at: String,
    pub prerelease: bool,
    pub pulls: Vec<IncludedPull>,
    pub pipeline: ReleasePipeline,
    pub state: PipelineState,
    pub succeeded: bool,
    pub complete: bool,
    pub blockers: Vec<PipelineBlockerReport>,
}

impl Release {
    pub(crate) fn observed(
        source: &ConnectedSource,
        origin: &str,
        release: &abnegate_vcs::Release,
        pulls: Vec<IncludedPull>,
        pipeline: ReleasePipeline,
    ) -> Self {
        Self {
            source_id: source.source.id,
            repository: source.configuration.full_name(),
            repository_url: source.configuration.web_url(origin),
            id: release.id,
            tag: release.tag.clone(),
            name: release.name.clone(),
            url: release.url.clone(),
            published_at: pipeline.release.published_at.clone(),
            prerelease: release.prerelease,
            pulls,
            state: pipeline.state(),
            succeeded: pipeline.succeeded(),
            complete: pipeline.complete(),
            blockers: pipeline.reports(),
            pipeline,
        }
    }

    pub fn with_pipeline(self, pipeline: ReleasePipeline) -> Self {
        Self {
            state: pipeline.state(),
            succeeded: pipeline.succeeded(),
            complete: pipeline.complete(),
            blockers: pipeline.reports(),
            pipeline,
            ..self
        }
    }
}

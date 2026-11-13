use serde::Serialize;

use crate::services::github::release::PipelineBlockerReport;
use crate::services::github::release::PipelineState;
use crate::services::github::release::ReleasePipeline;

#[derive(Debug, Serialize)]
pub struct PipelineResponse {
    pub pipeline: ReleasePipeline,
    pub state: PipelineState,
    pub succeeded: bool,
    pub complete: bool,
    pub blockers: Vec<PipelineBlockerReport>,
}

impl From<ReleasePipeline> for PipelineResponse {
    fn from(pipeline: ReleasePipeline) -> Self {
        Self {
            state: pipeline.state(),
            succeeded: pipeline.succeeded(),
            complete: pipeline.complete(),
            blockers: pipeline.reports(),
            pipeline,
        }
    }
}

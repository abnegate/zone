use crate::services::comfy::language;
use abnegate_comfy::host::Compute;
use abnegate_comfy::lora::TrainMethod;
use abnegate_comfy::lora::TrainRequest;
use abnegate_secret::SecretValue;

/// A run that passed validation, on its way to the background task that
/// trains it: an image request the crate trains, or chat documents zone
/// trains on the host with the compute and key it was placed on.
pub enum Training {
    Image(TrainRequest),
    Language {
        request: language::Request,
        compute: Compute,
        key: Option<SecretValue>,
    },
}

impl Training {
    pub fn name(&self) -> &str {
        match self {
            Self::Image(request) => &request.name,
            Self::Language { request, .. } => &request.name,
        }
    }

    pub fn method(&self) -> TrainMethod {
        match self {
            Self::Image(request) => request.method,
            Self::Language { request, .. } => request.method,
        }
    }

    pub fn compute(&self) -> Compute {
        match self {
            Self::Image(request) => request.compute,
            Self::Language { compute, .. } => *compute,
        }
    }
}

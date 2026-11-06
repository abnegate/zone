/// An `owner/repository:filename.safetensors` weight reference whose parts are
/// safe to put in a HuggingFace URL and to name a file under `loras/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComfyReference {
    repository: String,
    filename: String,
}

impl ComfyReference {
    pub(super) fn new(repository: &str, filename: &str) -> Self {
        Self {
            repository: repository.to_string(),
            filename: filename.to_string(),
        }
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn url(&self, hub_origin: &str) -> String {
        format!(
            "{}/{}/resolve/main/{}",
            hub_origin.trim_end_matches('/'),
            self.repository,
            self.filename
        )
    }
}

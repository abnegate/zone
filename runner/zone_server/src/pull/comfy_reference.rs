use reqwest::Url;

const RESOLVE: &str = "resolve";
const REVISION: &str = "main";

/// An `owner/repository:filename.safetensors` weight reference whose owner and
/// repository are HuggingFace id segments and whose filename is a single
/// printable path segment, safe to name a file under `loras/`.
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

    /// The file's download URL under `hub_origin`, or `None` when the origin
    /// is not a URL a path can be added to.
    pub fn url(&self, hub_origin: &str) -> Option<Url> {
        let mut url = Url::parse(hub_origin).ok()?;
        url.path_segments_mut()
            .ok()?
            .pop_if_empty()
            .extend(self.repository.split('/'))
            .extend([RESOLVE, REVISION, self.filename.as_str()]);
        Some(url)
    }
}

/// Whether `character` may appear in a HuggingFace owner or repository
/// segment.
pub(super) fn is_hub_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

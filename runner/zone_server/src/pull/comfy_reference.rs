use reqwest::Url;

const RESOLVE: &str = "resolve";
const REVISION: &str = "main";
const SEPARATOR: char = ':';
const EXTENSION: &str = ".safetensors";
const REPOSITORY_SEGMENTS: usize = 2;
const NAME_MAX: usize = 255;
/// The longest suffix abnegate-llm's downloader gives a sibling of the file
/// it writes: `.part`, `.part.lock` and `.part.validator`, whose names the
/// crate does not export.
const LONGEST_TRANSFER_SUFFIX: &str = ".part.validator";
const BIDIRECTIONAL_FORMATTING: [char; 12] = [
    '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

pub(super) const MISSING_SEPARATOR: &str = "Image weights need owner/repo:filename.safetensors";
pub(super) const INVALID_FILENAME: &str = "Invalid image weight filename";
pub(super) const FILENAME_TOO_LONG: &str = "Image weight filename is too long";
pub(super) const INVALID_REPOSITORY: &str = "Image weights need an owner/repo HuggingFace id";

/// An `owner/repository:filename.safetensors` weight reference whose owner and
/// repository are HuggingFace id segments and whose filename is a single
/// printable path segment, safe to name a file under `loras/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComfyReference {
    repository: String,
    filename: String,
}

impl ComfyReference {
    /// The reference `model` names, or why it names none.
    pub fn parse(model: &str) -> Result<Self, String> {
        let (repository, filename) = model
            .trim()
            .rsplit_once(SEPARATOR)
            .ok_or_else(|| MISSING_SEPARATOR.to_string())?;
        if repository.is_empty() || repository.contains("..") || !is_filename(filename) {
            return Err(INVALID_FILENAME.to_string());
        }
        if filename.len() + LONGEST_TRANSFER_SUFFIX.len() > NAME_MAX {
            return Err(FILENAME_TOO_LONG.to_string());
        }
        if !is_repository(repository) {
            return Err(INVALID_REPOSITORY.to_string());
        }
        Ok(Self {
            repository: repository.to_string(),
            filename: filename.to_string(),
        })
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

fn is_filename(filename: &str) -> bool {
    filename.ends_with(EXTENSION)
        && !filename.contains(['/', '\\'])
        && !filename.contains("..")
        && !filename.chars().any(|character| {
            character.is_control() || BIDIRECTIONAL_FORMATTING.contains(&character)
        })
}

fn is_repository(repository: &str) -> bool {
    repository.split('/').count() == REPOSITORY_SEGMENTS
        && repository.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.chars().all(is_hub_character)
        })
}

/// Whether `character` may appear in a HuggingFace owner or repository
/// segment.
fn is_hub_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONGEST_FILENAME_BYTES: usize = NAME_MAX - LONGEST_TRANSFER_SUFFIX.len();

    fn filename_of(bytes: usize) -> String {
        format!("{}{EXTENSION}", "a".repeat(bytes - EXTENSION.len()))
    }

    #[test]
    fn parses_owner_repo_safetensors_refs() {
        let reference = ComfyReference::parse(
            "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora:qwen-image-edit-plus-nsfw-lora.safetensors",
        )
        .unwrap();
        assert_eq!(
            reference,
            ComfyReference {
                repository: "ScottzillaSystems/qwen-image-edit-plus-nsfw-lora".to_string(),
                filename: "qwen-image-edit-plus-nsfw-lora.safetensors".to_string(),
            }
        );
        assert_eq!(
            reference.url("https://huggingface.co/").unwrap().as_str(),
            "https://huggingface.co/ScottzillaSystems/qwen-image-edit-plus-nsfw-lora/resolve/main/qwen-image-edit-plus-nsfw-lora.safetensors"
        );
        assert!(ComfyReference::parse("llama3.2:3b").is_err());
        assert!(ComfyReference::parse("../evil:model.safetensors").is_err());
        assert!(ComfyReference::parse("owner/repo:../model.safetensors").is_err());
        assert!(ComfyReference::parse("owner/repo/extra:model.safetensors").is_err());
        assert_eq!(
            ComfyReference::parse("owner/repo"),
            Err(MISSING_SEPARATOR.to_string())
        );
    }

    #[test]
    fn a_reference_keeps_the_hub_origins_path_and_ends_in_its_file() {
        let reference =
            ComfyReference::parse(" Owner-1/style_v2.0:style_v2.0-fp8.safetensors ").unwrap();
        assert_eq!(
            reference.url("https://mirror.example/hf").unwrap().as_str(),
            "https://mirror.example/hf/Owner-1/style_v2.0/resolve/main/style_v2.0-fp8.safetensors"
        );
        assert_eq!(reference.url("not a url"), None);
        assert_eq!(reference.url("mailto:hub@example.com"), None);
    }

    #[test]
    fn an_owner_or_repository_outside_huggingface_id_characters_is_refused() {
        for character in [
            '?', '#', '%', ' ', '\t', '\n', '&', '=', '+', ';', '@', '~', 'é', '\\',
        ] {
            assert_eq!(
                ComfyReference::parse(&format!("own{character}er/repo:style.safetensors")),
                Err(INVALID_REPOSITORY.to_string()),
                "{character:?} in the owner"
            );
            assert_eq!(
                ComfyReference::parse(&format!("owner/re{character}po:style.safetensors")),
                Err(INVALID_REPOSITORY.to_string()),
                "{character:?} in the repository"
            );
        }
        for repository in ["own:er/repo", "owner//repo", "./repo"] {
            assert_eq!(
                ComfyReference::parse(&format!("{repository}:style.safetensors")),
                Err(INVALID_REPOSITORY.to_string()),
                "{repository}"
            );
        }
    }

    #[test]
    fn a_filename_with_a_control_character_is_refused() {
        for character in ['\n', '\t', '\r', '\0', '\u{7f}', '\u{85}'] {
            assert_eq!(
                ComfyReference::parse(&format!("owner/repo:evil{character}.safetensors")),
                Err(INVALID_FILENAME.to_string()),
                "{character:?} in the filename"
            );
        }
    }

    #[test]
    fn a_filename_with_a_bidirectional_formatting_character_is_refused() {
        for character in BIDIRECTIONAL_FORMATTING {
            assert_eq!(
                ComfyReference::parse(&format!("owner/repo:evil{character}.safetensors")),
                Err(INVALID_FILENAME.to_string()),
                "{} in the filename",
                character.escape_unicode()
            );
        }
        for character in ['\u{200B}', '\u{2065}', '\u{202F}'] {
            assert!(
                ComfyReference::parse(&format!("owner/repo:style{character}.safetensors")).is_ok(),
                "{} is not a bidirectional formatting character",
                character.escape_unicode()
            );
        }
    }

    #[test]
    fn a_filename_whose_transfer_files_would_pass_name_max_is_refused() {
        let longest = filename_of(LONGEST_FILENAME_BYTES);
        let reference = ComfyReference::parse(&format!("owner/repo:{longest}")).unwrap();
        assert_eq!(
            format!("{}{LONGEST_TRANSFER_SUFFIX}", reference.filename()).len(),
            NAME_MAX
        );
        assert_eq!(
            ComfyReference::parse(&format!(
                "owner/repo:{}",
                filename_of(LONGEST_FILENAME_BYTES + 1)
            )),
            Err(FILENAME_TOO_LONG.to_string())
        );
    }

    #[test]
    fn a_filename_is_measured_in_bytes_not_characters() {
        let accented = format!(
            "{}{EXTENSION}",
            "é".repeat((LONGEST_FILENAME_BYTES - EXTENSION.len()) / 'é'.len_utf8() + 1)
        );
        assert!(accented.chars().count() <= LONGEST_FILENAME_BYTES);
        assert_eq!(
            ComfyReference::parse(&format!("owner/repo:{accented}")),
            Err(FILENAME_TOO_LONG.to_string())
        );
    }

    #[test]
    fn a_printable_filename_is_kept_and_percent_encoded_in_its_url() {
        const ORIGIN: &str = "https://huggingface.co/";
        const PREFIX: &str = "https://huggingface.co/owner/repo/resolve/main/";
        for (filename, encoded) in [
            ("evil#.safetensors", "evil%23.safetensors"),
            ("evil?.safetensors", "evil%3F.safetensors"),
            ("100%.safetensors", "100%25.safetensors"),
            ("My Lora (v2).safetensors", "My%20Lora%20(v2).safetensors"),
            ("style+detail.safetensors", "style+detail.safetensors"),
            ("estilo-é.safetensors", "estilo-%C3%A9.safetensors"),
        ] {
            let reference = ComfyReference::parse(&format!("owner/repo:{filename}"))
                .unwrap_or_else(|message| panic!("{filename:?} was refused: {message}"));
            assert_eq!(reference.filename(), filename);
            assert_eq!(
                reference.url(ORIGIN).unwrap().as_str(),
                format!("{PREFIX}{encoded}"),
                "{filename:?}"
            );
        }
    }
}

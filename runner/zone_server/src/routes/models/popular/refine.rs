use std::cmp::Ordering;
use std::cmp::Reverse;

use abnegate_llm::catalog::BrowseQuery;
use abnegate_llm::catalog::CatalogError;
use abnegate_llm::catalog::ModelCapability;
use abnegate_llm::catalog::ModelEntry;
use abnegate_llm::catalog::ModelMediumFilter;
use abnegate_llm::catalog::ModelPage;
use abnegate_llm::catalog::ModelSizeFilter;
use abnegate_llm::catalog::ModelSort;

const OFFSET_PREFIX: &str = "offset:";
const PAGE_PREFIX: &str = "page:";
const BILLION: &str = "billion";
const MILLION: &str = "million";
const MILLIONS_PER_BILLION: f64 = 1000.0;
const SMALL_BELOW: f64 = 4.0;
const MEDIUM_BELOW: f64 = 16.0;
const LARGE_BELOW: f64 = 40.0;

/// The models `options` keeps, in the order it asks for.
pub(super) fn refine(mut models: Vec<ModelEntry>, options: &BrowseQuery<'_>) -> Vec<ModelEntry> {
    models.retain(|model| matches_query(model, options));
    sort(&mut models, options.sort);
    models
}

pub(super) fn paginate(models: Vec<ModelEntry>, offset: usize, limit: usize) -> ModelPage {
    let total = models.len();
    let mut page = ModelPage::default();
    page.models = models.into_iter().skip(offset).take(limit).collect();
    let next_offset = offset + page.models.len();
    page.next_cursor = (next_offset < total).then(|| format!("{OFFSET_PREFIX}{next_offset}"));
    page
}

/// The offset a browse cursor names: `offset:N`, `page:N` counted in pages of
/// `limit` from one, or a bare offset, as the crate's catalogues read them.
pub(super) fn cursor_offset(cursor: Option<&str>, limit: usize) -> Result<usize, CatalogError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    if let Some(offset) = cursor.strip_prefix(OFFSET_PREFIX) {
        return offset
            .parse()
            .map_err(|_| CatalogError::Parse("Invalid cursor offset format".to_string()));
    }
    if let Some(page) = cursor.strip_prefix(PAGE_PREFIX) {
        let page: usize = page
            .parse()
            .map_err(|_| CatalogError::Parse("Invalid cursor page format".to_string()))?;
        return Ok(page.saturating_sub(1).saturating_mul(limit));
    }
    cursor
        .parse()
        .map_err(|_| CatalogError::Parse(format!("Unknown cursor format: {cursor}")))
}

fn matches_query(model: &ModelEntry, options: &BrowseQuery<'_>) -> bool {
    if let Some(family) = options.family
        && !matches_family(model, family)
    {
        return false;
    }
    if options.size != ModelSizeFilter::All && !matches_size(model, options.size) {
        return false;
    }
    matches_medium(model, options.medium)
}

fn matches_family(model: &ModelEntry, family: &str) -> bool {
    let needle = family.to_lowercase();
    if needle.is_empty() {
        return true;
    }
    if let Some(details) = &model.details
        && let Some(model_family) = &details.family
        && model_family.to_lowercase().contains(&needle)
    {
        return true;
    }
    model.name.to_lowercase().contains(&needle)
}

fn matches_size(model: &ModelEntry, size: ModelSizeFilter) -> bool {
    let Some(billions) = parameter_billions(model) else {
        return false;
    };
    match size {
        ModelSizeFilter::Small => billions < SMALL_BELOW,
        ModelSizeFilter::Medium => (SMALL_BELOW..MEDIUM_BELOW).contains(&billions),
        ModelSizeFilter::Large => (MEDIUM_BELOW..LARGE_BELOW).contains(&billions),
        ModelSizeFilter::ExtraLarge => billions >= LARGE_BELOW,
        _ => true,
    }
}

fn matches_medium(model: &ModelEntry, medium: ModelMediumFilter) -> bool {
    use ModelCapability::*;

    let wanted: &[ModelCapability] = match medium {
        ModelMediumFilter::Text => {
            return match model.capabilities.as_deref() {
                None | Some([]) => true,
                Some(capabilities) => capabilities.iter().any(|capability| {
                    matches!(
                        capability,
                        Text | ImageInput | VideoInput | Audio | AudioInput | Tools | Reasoning
                    )
                }),
            };
        }
        ModelMediumFilter::Image => &[ImageInput, ImageGeneration],
        ModelMediumFilter::ImageGeneration => &[ImageGeneration],
        ModelMediumFilter::Video => &[VideoInput, VideoGeneration],
        ModelMediumFilter::Audio => &[Audio, AudioInput, AudioGeneration],
        ModelMediumFilter::Tools => &[Tools],
        ModelMediumFilter::Embeddings => &[Embeddings],
        ModelMediumFilter::Reasoning => &[Reasoning],
        _ => return true,
    };
    model.capabilities.as_deref().is_some_and(|capabilities| {
        capabilities
            .iter()
            .any(|capability| wanted.contains(capability))
    })
}

fn sort(models: &mut [ModelEntry], sort: ModelSort) {
    match sort {
        ModelSort::NameAscending => models.sort_by_key(|model| model.name.to_lowercase()),
        ModelSort::NameDescending => models.sort_by_key(|model| Reverse(model.name.to_lowercase())),
        ModelSort::DownloadsAscending => {
            models.sort_by(|left, right| ascending(left.downloads, right.downloads))
        }
        ModelSort::DownloadsDescending => {
            models.sort_by(|left, right| descending(left.downloads, right.downloads))
        }
        ModelSort::SizeAscending => models.sort_by(|left, right| ascending(left.size, right.size)),
        ModelSort::SizeDescending => {
            models.sort_by(|left, right| descending(left.size, right.size))
        }
        ModelSort::ParametersAscending => models
            .sort_by(|left, right| ascending(parameter_billions(left), parameter_billions(right))),
        ModelSort::ParametersDescending => models
            .sort_by(|left, right| descending(parameter_billions(left), parameter_billions(right))),
        ModelSort::UpdatedAscending => models.sort_by(|left, right| {
            ascending(left.modified_at.as_deref(), right.modified_at.as_deref())
        }),
        ModelSort::UpdatedDescending => models.sort_by(|left, right| {
            descending(left.modified_at.as_deref(), right.modified_at.as_deref())
        }),
        _ => {}
    }
}

/// Known values first, in order; unknown values last.
fn ascending<T: PartialOrd>(left: Option<T>, right: Option<T>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Known values first, in reverse order; unknown values still last.
fn descending<T: PartialOrd>(left: Option<T>, right: Option<T>) -> Ordering {
    if left.is_some() && right.is_some() {
        ascending(left, right).reverse()
    } else {
        ascending(left, right)
    }
}

fn parameter_billions(model: &ModelEntry) -> Option<f64> {
    model
        .details
        .as_ref()
        .and_then(|details| details.parameter_size.as_deref())
        .and_then(parse_billions)
}

/// Billions of parameters in a label such as `3.8B`, `137M` or `7 billion`.
fn parse_billions(raw: &str) -> Option<f64> {
    let lower = raw.to_lowercase();
    if lower.contains(BILLION) {
        return last_number_before(&lower, BILLION);
    }
    if lower.contains(MILLION) {
        return last_number_before(&lower, MILLION).map(|millions| millions / MILLIONS_PER_BILLION);
    }

    let mut number = String::new();
    let mut unit = None::<char>;
    for character in lower.chars() {
        if character.is_ascii_digit() || (character == '.' && !number.contains('.')) {
            number.push(character);
            continue;
        }
        if !number.is_empty() {
            if character == 'b' || character == 'm' {
                unit = Some(character);
            }
            break;
        }
    }
    if number.is_empty() {
        return None;
    }

    let value: f64 = number.parse().ok()?;
    match unit {
        Some('m') => Some(value / MILLIONS_PER_BILLION),
        _ => Some(value),
    }
}

fn last_number_before(text: &str, word: &str) -> Option<f64> {
    text.split(word)
        .next()?
        .split_whitespace()
        .rev()
        .find_map(|token| {
            token
                .trim_matches(|character: char| !character.is_ascii_digit() && character != '.')
                .parse()
                .ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use abnegate_llm::catalog::ModelDetails;

    fn model(name: &str, family: &str, parameter_size: &str, size: Option<u64>) -> ModelEntry {
        let mut details = ModelDetails::default();
        details.family = Some(family.to_string());
        details.parameter_size = Some(parameter_size.to_string());
        let mut entry = ModelEntry::default();
        entry.name = name.to_string();
        entry.size = size;
        entry.details = Some(details);
        entry
    }

    fn names(models: &[ModelEntry]) -> Vec<&str> {
        models.iter().map(|model| model.name.as_str()).collect()
    }

    #[test]
    fn millions_and_billions_read_as_billions() {
        assert_eq!(parse_billions("3.8B"), Some(3.8));
        assert_eq!(parse_billions("137M"), Some(0.137));
        assert_eq!(parse_billions("7 billion"), Some(7.0));
        assert_eq!(parse_billions("335 million"), Some(0.335));
        assert_eq!(parse_billions("70"), Some(70.0));
        assert_eq!(parse_billions("unknown"), None);
    }

    #[test]
    fn a_family_matches_by_substring_of_family_or_name() {
        let codellama = model("codellama", "codellama", "7B", None);
        assert!(matches_family(&codellama, "llama"));
        assert!(matches_family(&codellama, "CODE"));
        assert!(!matches_family(&codellama, "qwen"));
        assert!(matches_family(&codellama, ""));
    }

    #[test]
    fn size_buckets_follow_parameter_billions() {
        let embedder = model("nomic-embed-text", "nomic", "137M", None);
        let medium = model("llama3.1", "llama", "8B", None);
        let large = model("yi", "yi", "34B", None);
        let huge = model("llama3.1:70b", "llama", "70B", None);
        assert!(matches_size(&embedder, ModelSizeFilter::Small));
        assert!(matches_size(&medium, ModelSizeFilter::Medium));
        assert!(matches_size(&large, ModelSizeFilter::Large));
        assert!(matches_size(&huge, ModelSizeFilter::ExtraLarge));
        assert!(!matches_size(&huge, ModelSizeFilter::Small));
    }

    #[test]
    fn a_capability_medium_needs_declared_capabilities() {
        let plain = model("mistral", "mistral", "7B", None);
        assert!(matches_medium(&plain, ModelMediumFilter::All));
        assert!(matches_medium(&plain, ModelMediumFilter::Text));
        assert!(!matches_medium(&plain, ModelMediumFilter::Embeddings));

        let mut embedder = plain.clone();
        embedder.capabilities = Some(vec![ModelCapability::Embeddings]);
        assert!(matches_medium(&embedder, ModelMediumFilter::Embeddings));
        assert!(!matches_medium(&embedder, ModelMediumFilter::Text));
    }

    #[test]
    fn unknown_sizes_sort_last_either_way() {
        let models = vec![
            model("unsized", "llama", "1B", None),
            model("large", "llama", "8B", Some(8)),
            model("small", "llama", "3B", Some(3)),
        ];
        let mut options = BrowseQuery::default();
        options.sort = ModelSort::SizeAscending;
        assert_eq!(
            names(&refine(models.clone(), &options)),
            ["small", "large", "unsized"]
        );
        options.sort = ModelSort::SizeDescending;
        assert_eq!(
            names(&refine(models, &options)),
            ["large", "small", "unsized"]
        );
    }

    #[test]
    fn refine_filters_then_sorts() {
        let models = vec![
            model("qwen2.5", "qwen", "7B", None),
            model("llama3.2", "llama", "3B", None),
            model("codellama", "codellama", "7B", None),
        ];
        let mut options = BrowseQuery::default();
        options.family = Some("llama");
        options.sort = ModelSort::NameAscending;
        assert_eq!(names(&refine(models, &options)), ["codellama", "llama3.2"]);
    }

    #[test]
    fn a_page_names_the_offset_after_it_while_models_remain() {
        let models: Vec<ModelEntry> = (0..21)
            .map(|index| model(&format!("model-{index}"), "llama", "1B", None))
            .collect();
        let first = paginate(models.clone(), 0, 20);
        assert_eq!(first.models.len(), 20);
        assert_eq!(first.next_cursor.as_deref(), Some("offset:20"));
        let last = paginate(models.clone(), 20, 20);
        assert_eq!(names(&last.models), ["model-20"]);
        assert_eq!(last.next_cursor, None);
        assert_eq!(paginate(models, 30, 20).models.len(), 0);
    }

    #[test]
    fn cursors_read_offsets_pages_and_bare_numbers() {
        assert_eq!(cursor_offset(None, 5).unwrap(), 0);
        assert_eq!(cursor_offset(Some("offset:5"), 5).unwrap(), 5);
        assert_eq!(cursor_offset(Some("page:3"), 5).unwrap(), 10);
        assert_eq!(cursor_offset(Some("page:0"), 5).unwrap(), 0);
        assert_eq!(cursor_offset(Some("7"), 5).unwrap(), 7);
        assert!(cursor_offset(Some("offset:x"), 5).is_err());
        assert!(cursor_offset(Some("page:x"), 5).is_err());
        assert!(cursor_offset(Some("next"), 5).is_err());
    }
}

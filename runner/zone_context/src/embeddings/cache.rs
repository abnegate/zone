//! Where in-process models keep their downloaded weights.

use std::path::PathBuf;

/// Environment variable overriding the on-disk model cache directory.
pub const DIRECTORY_ENVIRONMENT: &str = "ZONE_EMBED_CACHE_DIR";

const APPLICATION_DIRECTORY: &str = "zone";
const ENGINE_DIRECTORY: &str = "fastembed";
const WORKING_DIRECTORY_FALLBACK: &str = ".fastembed_cache";

/// `$ZONE_EMBED_CACHE_DIR` when set, else `<os cache dir>/zone/fastembed`,
/// else `.fastembed_cache` in the working directory.
pub fn default_directory() -> PathBuf {
    if let Ok(directory) = std::env::var(DIRECTORY_ENVIRONMENT)
        && !directory.trim().is_empty()
    {
        return PathBuf::from(directory);
    }
    operating_system_directory()
}

fn operating_system_directory() -> PathBuf {
    dirs::cache_dir()
        .map(|cache| cache.join(APPLICATION_DIRECTORY).join(ENGINE_DIRECTORY))
        .unwrap_or_else(|| PathBuf::from(WORKING_DIRECTORY_FALLBACK))
}

#[cfg(test)]
mod tests {
    use zone_core::variables::Variables;

    use super::*;

    const OVERRIDE: &str = "/tmp/zone-embed-test";
    const BLANK: &str = "  ";

    #[test]
    fn the_environment_overrides_the_default_directory() {
        let mut variables = Variables::lock();
        variables.set(DIRECTORY_ENVIRONMENT, OVERRIDE);
        assert_eq!(default_directory(), PathBuf::from(OVERRIDE));
        variables.remove(DIRECTORY_ENVIRONMENT);
        assert_eq!(default_directory(), operating_system_directory());
    }

    #[test]
    fn a_blank_override_keeps_the_operating_system_directory() {
        let mut variables = Variables::lock();
        variables.set(DIRECTORY_ENVIRONMENT, BLANK);
        let directory = default_directory();
        variables.remove(DIRECTORY_ENVIRONMENT);
        assert_eq!(directory, operating_system_directory());
    }

    #[test]
    fn the_operating_system_directory_is_zone_fastembed_under_the_os_cache() {
        let Some(cache) = dirs::cache_dir() else {
            assert_eq!(
                operating_system_directory(),
                PathBuf::from(WORKING_DIRECTORY_FALLBACK)
            );
            return;
        };
        assert_eq!(
            operating_system_directory(),
            cache.join(APPLICATION_DIRECTORY).join(ENGINE_DIRECTORY)
        );
    }
}

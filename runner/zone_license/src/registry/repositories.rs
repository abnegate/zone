use crate::registry::name;
use crate::registry::repositories_error::RepositoriesError;

pub const DEFAULT_REPOSITORIES: [&str; 3] = ["zone/server", "zone/manager", "zone/litellm"];

const LIST_SEPARATOR: char = ',';

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repositories(Vec<String>);

impl Repositories {
    pub fn parse(list: &str) -> Result<Self, RepositoriesError> {
        let mut repositories: Vec<String> = Vec::new();
        for entry in list
            .split(LIST_SEPARATOR)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if !name::repository(entry) {
                return Err(RepositoriesError::Invalid {
                    entry: entry.to_owned(),
                });
            }
            if repositories.iter().any(|known| known == entry) {
                return Err(RepositoriesError::Duplicate {
                    entry: entry.to_owned(),
                });
            }
            repositories.push(entry.to_owned());
        }
        if repositories.is_empty() {
            return Err(RepositoriesError::Empty);
        }
        Ok(Self(repositories))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|repository| repository == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

impl Default for Repositories {
    fn default() -> Self {
        Self(DEFAULT_REPOSITORIES.map(str::to_owned).to_vec())
    }
}

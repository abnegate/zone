#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepositoriesError {
    #[error("it names no repository")]
    Empty,
    #[error("{entry:?} is not a repository path such as zone/server")]
    Invalid { entry: String },
    #[error("{entry:?} is listed twice")]
    Duplicate { entry: String },
}

use super::Release;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub releases: Vec<Release>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

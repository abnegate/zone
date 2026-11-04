/// How hard to look for a repository's checkout in a workspace's folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    /// Walk the folders when no checkout is known yet.
    Walk,
    /// Use only a checkout already known, still where it was and still of that repository.
    Known,
}

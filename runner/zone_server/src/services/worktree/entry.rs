//! One linked worktree as `git worktree list --porcelain -z` describes it.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub head: String,
    pub locked: bool,
}

impl Entry {
    /// Every worktree in a listing that has a commit checked out. The repository's own checkout
    /// is listed first and is never linked, so it is left out; so are bare entries and any on an
    /// unborn branch, which have no commit to judge.
    pub fn parse(listing: &[u8]) -> Vec<Self> {
        let text = String::from_utf8_lossy(listing);
        text.split("\0\0")
            .skip(1)
            .filter_map(|record| {
                let mut path = None;
                let mut head = None;
                let mut locked = false;
                for field in record.split('\0') {
                    let (key, value) = field.split_once(' ').unwrap_or((field, ""));
                    match key {
                        "worktree" => path = Some(PathBuf::from(value)),
                        "HEAD" => head = Some(value.to_string()),
                        "locked" => locked = true,
                        "bare" => return None,
                        _ => {}
                    }
                }
                Some(Self {
                    path: path?,
                    head: head.filter(|head| head.bytes().any(|byte| byte != b'0'))?,
                    locked,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "1f0c2a5a8c6c3b8c4f0d3e7b2f6a9d1e5c7b3a90";

    #[test]
    fn reads_every_linked_worktree_but_not_the_repository_itself() {
        let listing = format!(
            "worktree /repo\0HEAD {HEAD}\0branch refs/heads/main\0\0\
             worktree /repo/.worktrees/fix\0HEAD {HEAD}\0branch refs/heads/fix\0\0\
             worktree /repo/.worktrees/held\0HEAD {HEAD}\0detached\0locked moving disks\0\0\
             worktree /repo/.worktrees/empty\0HEAD 0000000000000000000000000000000000000000\0\
             branch refs/heads/empty\0\0"
        );

        assert_eq!(
            Entry::parse(listing.as_bytes()),
            vec![
                Entry {
                    path: PathBuf::from("/repo/.worktrees/fix"),
                    head: HEAD.to_string(),
                    locked: false,
                },
                Entry {
                    path: PathBuf::from("/repo/.worktrees/held"),
                    head: HEAD.to_string(),
                    locked: true,
                },
            ]
        );
    }
}

use std::fmt;

const SCHEMES: [&str; 5] = ["https", "http", "ssh", "git", "git+ssh"];
const GIT_SUFFIX: &str = ".git";

/// A remote repository as git's several spellings of it all name it: the host and the path,
/// lowercased because hosts treat them without regard to case, with no scheme, user, password,
/// port or `.git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    host: String,
    path: String,
}

impl Origin {
    /// `remote` as a network URL or as git's `[user@]host:path`. Nothing for a local path, or a
    /// URL with no owner in its path, which name nothing another checkout could be compared to.
    pub fn parse(remote: &str) -> Option<Self> {
        let remote = remote.trim();
        let (authority, path) = match remote.split_once("://") {
            Some((scheme, rest)) => {
                if !SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) {
                    return None;
                }
                rest.split_once('/')?
            }
            None => {
                let (authority, path) = remote.split_once(':')?;
                if authority.contains('/') {
                    return None;
                }
                (authority, path)
            }
        };
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = host.split_once(':').map_or(host, |(host, _)| host);
        let path = path.trim_matches('/');
        let path = path.strip_suffix(GIT_SUFFIX).unwrap_or(path);
        let named = !host.is_empty()
            && path.contains('/')
            && !path.contains(['?', '#', '\\'])
            && path.split('/').all(|segment| !segment.is_empty());
        named.then(|| Self {
            host: host.to_ascii_lowercase(),
            path: path.to_ascii_lowercase(),
        })
    }

    /// The host-relative path, which appears in any spelling of this remote.
    pub(super) fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.host, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected() -> Origin {
        Origin::parse("https://github.com/owner/repository").expect("a repository")
    }

    #[test]
    fn every_spelling_of_a_remote_is_the_same_origin() {
        for remote in [
            "https://github.com/owner/repository",
            "https://github.com/owner/repository.git",
            "https://github.com/owner/repository/",
            "https://GitHub.com/OWNER/Repository.git",
            "http://github.com/owner/repository.git",
            "https://user:hunter2@github.com/owner/repository.git",
            "git@github.com:owner/repository.git",
            "git@github.com:owner/repository",
            "github.com:owner/repository.git",
            "ssh://git@github.com/owner/repository.git",
            "ssh://git@github.com:22/owner/repository.git",
            "git://github.com/owner/repository.git",
            "git+ssh://git@github.com/owner/repository",
            "  https://github.com/owner/repository.git\n",
        ] {
            assert_eq!(Origin::parse(remote), Some(expected()), "{remote}");
        }
    }

    #[test]
    fn another_host_owner_or_name_is_another_origin() {
        for remote in [
            "https://gitlab.com/owner/repository.git",
            "https://github.com/other/repository.git",
            "https://github.com/owner/repository-fork.git",
            "git@github.example:owner/repository.git",
        ] {
            assert_ne!(Origin::parse(remote), Some(expected()), "{remote}");
        }
    }

    #[test]
    fn a_local_path_or_a_remote_with_no_owner_names_no_origin() {
        for remote in [
            "",
            "/srv/git/repository.git",
            "../repository.git",
            "./a:b/repository.git",
            "file:///srv/git/repository.git",
            "https://github.com/repository",
            "https://github.com",
            "C:\\work\\repository",
            "https://github.com/owner/repository?token=secret",
        ] {
            assert_eq!(Origin::parse(remote), None, "{remote:?}");
        }
    }

    #[test]
    fn a_nested_group_keeps_its_whole_path() {
        let origin = Origin::parse("https://gitlab.example/group/subgroup/project.git");
        assert_eq!(
            origin.map(|origin| origin.to_string()).as_deref(),
            Some("gitlab.example/group/subgroup/project")
        );
    }
}

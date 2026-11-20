//! Whether a commit's work has reached the default branch, merged or squashed.

use std::io;
use std::path::Path;

use super::git;

/// The same diff whatever the repository configures, so two patches of the same change get the
/// same patch id: no rename detection, no external diff or text conversion, fixed prefixes.
const PATCH: [&str; 6] = [
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Whether `head` is an ancestor of `target`, or the whole of its change since the two diverged
/// landed on `target` as one commit, which is what a squash merge leaves behind.
pub async fn merged(directory: &Path, head: &str, target: &str) -> io::Result<bool> {
    if git::answers(directory, &["merge-base", "--is-ancestor", head, target]).await? {
        return Ok(true);
    }
    let base = git::output(directory, &["merge-base", head, target]).await?;
    if !base.status.success() {
        return Ok(false);
    }
    let base = String::from_utf8_lossy(&base.stdout).trim().to_string();

    let mut change = vec!["diff"];
    change.extend(PATCH);
    change.extend([base.as_str(), head]);
    let Some(change) = git::patch_ids(directory, &change).await?.into_iter().next() else {
        return Ok(false);
    };

    let since = format!("{base}..{target}");
    let mut landed = vec!["log", "-p", "--no-merges", "--format=commit %H"];
    landed.extend(PATCH);
    landed.push(&since);
    Ok(git::patch_ids(directory, &landed).await?.contains(&change))
}

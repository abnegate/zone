use abnegate_vcs::MergeMethod;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    #[default]
    Merge,
    Squash,
    Rebase,
}

impl From<Method> for MergeMethod {
    fn from(method: Method) -> Self {
        match method {
            Method::Merge => MergeMethod::Merge,
            Method::Squash => MergeMethod::Squash,
            Method::Rebase => MergeMethod::Rebase,
        }
    }
}

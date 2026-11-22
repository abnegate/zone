use serde::Serialize;

use crate::bundle::{Architecture, BundleVersion, DOWNLOAD_LIFETIME, Object, Presigner};
use crate::http::bundle_link::BundleLink;

/// Every URL in it lets anyone holding it download that file until it expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleResponse {
    pub version: String,
    pub architecture: &'static str,
    pub expires_in: u64,
    pub files: [BundleLink; 3],
}

impl BundleResponse {
    pub fn presign(
        presigner: &Presigner,
        version: BundleVersion,
        architecture: Architecture,
    ) -> Self {
        Self {
            version: version.to_string(),
            architecture: architecture.as_str(),
            expires_in: DOWNLOAD_LIFETIME.as_secs(),
            files: Object::files(version, architecture).map(|object| BundleLink {
                name: object.name(),
                url: presigner.presign(&object, DOWNLOAD_LIFETIME).into(),
            }),
        }
    }
}

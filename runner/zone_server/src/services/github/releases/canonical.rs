use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;

use super::IncludedPull;

#[derive(Serialize)]
pub(crate) struct Canonical<'a> {
    repository: String,
    tag: &'a str,
    name: &'a str,
    body: &'a str,
    base_tag: Option<&'a str>,
    target: String,
    pulls: &'a [IncludedPull],
}

impl<'a> Canonical<'a> {
    pub(crate) fn new(
        repository: &str,
        tag: &'a str,
        name: &'a str,
        body: &'a str,
        base_tag: Option<&'a str>,
        target: &str,
        pulls: &'a [IncludedPull],
    ) -> Self {
        Self {
            repository: repository.to_lowercase(),
            tag,
            name,
            body,
            base_tag,
            target: target.to_lowercase(),
            pulls,
        }
    }

    pub(crate) fn digest(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("a preview serialises to JSON");
        hex::encode(Sha256::digest(canonical))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "ABCDEFabcdef0123456789abcdef0123456789ab";

    fn pulls() -> Vec<IncludedPull> {
        vec![IncludedPull {
            number: 12,
            url: "https://github.com/acme/widgets/pull/12".into(),
            title: "Add widgets".into(),
        }]
    }

    fn digested(repository: &str, body: &str, target: &str, pulls: &[IncludedPull]) -> String {
        Canonical::new(
            repository,
            "v1.4.1",
            "v1.4.1",
            body,
            Some("v1.4.0"),
            target,
            pulls,
        )
        .digest()
    }

    #[test]
    fn the_same_preview_digests_the_same_whatever_the_case_of_its_repository_or_target() {
        let pulls = pulls();
        let digest = digested("acme/widgets", "notes", TARGET, &pulls);
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            digest,
            digested("Acme/Widgets", "notes", &TARGET.to_lowercase(), &pulls)
        );
    }

    #[test]
    fn any_change_to_what_would_be_published_changes_the_digest() {
        let pulls = pulls();
        let digest = digested("acme/widgets", "notes", TARGET, &pulls);
        let mut retitled = pulls.clone();
        retitled[0].title = "Add gadgets".into();
        for changed in [
            digested("acme/gadgets", "notes", TARGET, &pulls),
            digested("acme/widgets", "notes!", TARGET, &pulls),
            digested("acme/widgets", "notes", &"b".repeat(40), &pulls),
            digested("acme/widgets", "notes", TARGET, &[]),
            digested("acme/widgets", "notes", TARGET, &retitled),
            Canonical::new(
                "acme/widgets",
                "v1.4.1",
                "v1.4.1",
                "notes",
                None,
                TARGET,
                &pulls,
            )
            .digest(),
            Canonical::new(
                "acme/widgets",
                "v1.4.2",
                "v1.4.1",
                "notes",
                Some("v1.4.0"),
                TARGET,
                &pulls,
            )
            .digest(),
            Canonical::new(
                "acme/widgets",
                "v1.4.1",
                "Widgets",
                "notes",
                Some("v1.4.0"),
                TARGET,
                &pulls,
            )
            .digest(),
        ] {
            assert_ne!(changed, digest);
        }
    }
}

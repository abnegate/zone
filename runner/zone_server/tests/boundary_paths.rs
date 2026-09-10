//! Path-boundary attacks against artifact serving, the artifact store, and the
//! attachment resolver that feeds image-to-image from chat metadata.
mod common;

use axum::http::StatusCode;
use common::context::Harness;
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;
use zone_server::services::artifacts::{ArtifactError, ArtifactStore};
use zone_server::services::media_source::{self, Error as MediaError};

const CLIP: &[u8] = b"\x1a\x45\xdf\xa3generated-video-artifact-payload";
const SECRET: &[u8] = b"ROOT-ESCAPE-SENTINEL-DO-NOT-SERVE";

/// An artifact root with a planted artifact and a secret one directory above it.
struct Rooted {
    harness: Harness,
    root: PathBuf,
    outside: PathBuf,
    owner: Uuid,
}

impl Rooted {
    async fn new() -> Self {
        let mut harness = Harness::new(Some(32768), false, vec![]).await;
        let parent = std::env::temp_dir().join(format!("zone-boundary-{}", Uuid::new_v4()));
        let root = parent.join("artifacts");
        let outside = parent.join("outside");
        let owner = Uuid::new_v4();
        let directory = root
            .join(harness.workspace.to_string())
            .join(harness.chat.to_string())
            .join(owner.to_string());
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(directory.join("clip.webm"), CLIP).unwrap();
        std::fs::write(outside.join("secret.txt"), SECRET).unwrap();
        harness.config.comfyui.artifact_root = root.clone();
        harness.restart().await;
        Self {
            harness,
            root,
            outside,
            owner,
        }
    }

    fn store(&self) -> ArtifactStore {
        ArtifactStore::new(self.root.clone())
    }

    fn owner_directory(&self) -> PathBuf {
        self.root
            .join(self.harness.workspace.to_string())
            .join(self.harness.chat.to_string())
            .join(self.owner.to_string())
    }

    fn url(&self, filename: &str) -> String {
        format!(
            "/api/artifacts/{}/{}/{}/{}",
            self.harness.workspace, self.harness.chat, self.owner, filename
        )
    }
}

impl Drop for Rooted {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
}

/// Every spelling of "leave the artifact root" a URL can carry, executed
/// against the real authenticated route.
#[tokio::test]
async fn artifact_serving_refuses_every_traversal_spelling() {
    let fixture = Rooted::new().await;
    let sane = fixture
        .harness
        .client
        .get_auth(&fixture.url("clip.webm"), &fixture.harness.token)
        .await;
    sane.assert_status(StatusCode::OK);
    assert_eq!(
        sane.bytes(),
        CLIP,
        "the fixture artifact must be servable, or the attacks below prove nothing"
    );

    let long = "a".repeat(4096);
    let deep = "..%2f".repeat(40);
    for filename in [
        "..%2f..%2f..%2fsecret.txt",
        "%2e%2e%2f%2e%2e%2f%2e%2e%2fsecret.txt",
        "%252e%252e%252f%252e%252e%252fsecret.txt",
        "..%2f..%2foutside%2fsecret.txt",
        "..%5c..%5csecret.txt",
        "%2f..%2f..%2foutside%2fsecret.txt",
        "clip.webm%00.png",
        "..%00%2f..%2fsecret.txt",
        ".%2e%2f.%2e%2fsecret.txt",
        "%2e%2e%2F%2e%2e%2Fsecret.txt",
        "..;%2f..;%2fsecret.txt",
        long.as_str(),
        deep.as_str(),
        "%uff0e%uff0e%2fsecret.txt",
        "\u{ff0e}\u{ff0e}%2fsecret.txt",
        "..%c0%af..%c0%afsecret.txt",
    ] {
        let response = fixture
            .harness
            .client
            .get_auth(&fixture.url(filename), &fixture.harness.token)
            .await;
        assert_ne!(
            response.status,
            StatusCode::OK,
            "{filename} escaped the artifact root and was served"
        );
        assert!(
            !response.bytes().windows(SECRET.len()).any(|w| w == SECRET),
            "{filename} leaked content from above the artifact root"
        );
    }
}

/// A signature is minted over whatever string the caller names, so the store is
/// the only thing standing between a signed traversal URL and the filesystem.
#[tokio::test]
async fn a_signed_traversal_url_is_still_refused_by_the_store() {
    let fixture = Rooted::new().await;
    let minted = fixture
        .harness
        .client
        .get_auth(
            &format!("{}/signature", fixture.url("..%2f..%2fsecret.txt")),
            &fixture.harness.token,
        )
        .await;
    minted.assert_status(StatusCode::OK);
    let signed = minted.json_value();
    let signed = signed["url"].as_str().expect("a minted signature url");
    let response = fixture.harness.client.get(signed).await;
    assert_ne!(
        response.status,
        StatusCode::OK,
        "a signed traversal URL served {signed}"
    );
    assert!(
        !response.bytes().windows(SECRET.len()).any(|w| w == SECRET),
        "a signed traversal URL leaked content from above the artifact root"
    );
}

/// The guard has to resolve links before it opens anything: a name that is a
/// perfectly ordinary path component can still point outside the root.
#[tokio::test]
async fn a_symlink_out_of_the_artifact_root_is_refused() {
    let fixture = Rooted::new().await;
    std::os::unix::fs::symlink(
        fixture.outside.join("secret.txt"),
        fixture.owner_directory().join("link.webm"),
    )
    .unwrap();

    let opened = fixture
        .store()
        .open(
            fixture.harness.workspace,
            fixture.harness.chat,
            fixture.owner,
            "link.webm",
        )
        .await;
    assert!(
        matches!(opened, Err(ArtifactError::InvalidPath)),
        "a symlink pointing above the root must be refused, got {:?}",
        opened.map(|artifact| artifact.length())
    );

    let response = fixture
        .harness
        .client
        .get_auth(&fixture.url("link.webm"), &fixture.harness.token)
        .await;
    assert_ne!(
        response.status,
        StatusCode::OK,
        "the route served a symlink pointing above the artifact root"
    );
    assert!(
        !response.bytes().windows(SECRET.len()).any(|w| w == SECRET),
        "the route leaked a symlink target from above the artifact root"
    );
}

/// The same escape one level up: the per-message directory itself is the link.
#[tokio::test]
async fn a_symlinked_owner_directory_is_refused() {
    let fixture = Rooted::new().await;
    let owner = Uuid::new_v4();
    std::os::unix::fs::symlink(
        &fixture.outside,
        fixture
            .root
            .join(fixture.harness.workspace.to_string())
            .join(fixture.harness.chat.to_string())
            .join(owner.to_string()),
    )
    .unwrap();

    let opened = fixture
        .store()
        .open(
            fixture.harness.workspace,
            fixture.harness.chat,
            owner,
            "secret.txt",
        )
        .await;
    assert!(
        matches!(opened, Err(ArtifactError::InvalidPath)),
        "a symlinked owner directory must be refused, got {:?}",
        opened.map(|artifact| artifact.length())
    );
}

/// A root reached through a link is the normal case on macOS, where the whole
/// temporary directory is one, so the guard must resolve rather than compare.
#[tokio::test]
async fn a_symlinked_root_still_serves_its_own_artifacts() {
    let fixture = Rooted::new().await;
    let alias = fixture
        .root
        .parent()
        .expect("the fixture root has a parent")
        .join("alias");
    std::os::unix::fs::symlink(&fixture.root, &alias).unwrap();

    let bytes = ArtifactStore::new(alias)
        .read(
            fixture.harness.workspace,
            fixture.harness.chat,
            fixture.owner,
            "clip.webm",
        )
        .await
        .expect("an artifact under a symlinked root is still its own artifact");
    assert_eq!(bytes, CLIP);
}

/// Attachment URLs live in chat metadata, so they are written by whatever the
/// client or the model put there rather than by the server.
#[tokio::test]
async fn attachment_urls_cannot_walk_out_of_the_artifact_root() {
    let root = std::env::temp_dir().join(format!("zone-boundary-media-{}", Uuid::new_v4()));
    let outside = root.join("outside");
    let workspace = Uuid::new_v4();
    let chat = Uuid::new_v4();
    let owner = Uuid::new_v4();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), SECRET).unwrap();
    let store = ArtifactStore::new(root.join("artifacts"));

    for url in [
        "/api/artifacts/{workspace}/{chat}/{owner}/..%2f..%2fsecret.txt",
        "/api/artifacts/{workspace}/{chat}/{owner}/../../secret.txt",
        "/api/artifacts/{workspace}/{chat}/{owner}/../outside/secret.txt",
        "/api/artifacts/{workspace}/{chat}/{owner}/secret.txt/../secret.txt",
    ] {
        let url = url
            .replace("{workspace}", &workspace.to_string())
            .replace("{chat}", &chat.to_string())
            .replace("{owner}", &owner.to_string());
        let metadata = json!({"attachments":[{"name":"shot.png","mime":"image/png","url":url}]});
        let resolved =
            media_source::resolve_source_image(Some(&metadata), workspace, chat, &store).await;
        assert!(
            matches!(resolved, Err(MediaError::Unreadable)),
            "{url} must not resolve to bytes, got {:?}",
            resolved.map(|source| source.map(|image| image.bytes.len()))
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Image-to-image is not a fetch proxy: a remote attachment URL is refused
/// rather than requested, whatever it points at.
#[tokio::test]
async fn attachment_urls_are_never_fetched() {
    let store = ArtifactStore::new(std::env::temp_dir().join(format!("unused-{}", Uuid::new_v4())));
    let workspace = Uuid::new_v4();
    let chat = Uuid::new_v4();
    for url in [
        "http://127.0.0.1:11434/api/tags",
        "http://169.254.169.254/latest/meta-data/",
        "https://example.com/shot.png",
        "file:///etc/passwd",
        "/etc/passwd",
        "//example.com/shot.png",
    ] {
        let metadata = json!({"attachments":[{"name":"shot.png","mime":"image/png","url":url}]});
        let resolved =
            media_source::resolve_source_image(Some(&metadata), workspace, chat, &store).await;
        assert!(
            matches!(resolved, Err(MediaError::Unreadable)),
            "{url} must not be fetched, got {:?}",
            resolved.map(|source| source.map(|image| image.bytes.len()))
        );
    }
}

/// An attachment naming another workspace's artifact must not be read into this
/// chat's generation, even though the store itself would happily open it.
#[tokio::test]
async fn attachment_urls_cannot_reach_another_chat() {
    let root = std::env::temp_dir().join(format!("zone-boundary-cross-{}", Uuid::new_v4()));
    let store = ArtifactStore::new(root.clone());
    let workspace = Uuid::new_v4();
    let chat = Uuid::new_v4();
    let elsewhere = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let planted = store
        .persist(workspace, elsewhere, owner, "png", CLIP)
        .await
        .expect("the planted artifact is stored");
    let metadata = json!({"attachments":[{"name":"shot.png","mime":"image/png","url":planted}]});
    let resolved =
        media_source::resolve_source_image(Some(&metadata), workspace, chat, &store).await;
    assert!(
        matches!(resolved, Err(MediaError::Unreadable)),
        "an artifact from chat {elsewhere} must not resolve inside chat {chat}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// macOS resolves a stored name case-insensitively and Linux does not, so the
/// guard cannot depend on the spelling. Either answer is correct; reaching a
/// different owner's directory is not.
#[tokio::test]
async fn a_case_variant_name_stays_inside_the_owner_directory() {
    let fixture = Rooted::new().await;
    let response = fixture
        .harness
        .client
        .get_auth(&fixture.url("CLIP.WEBM"), &fixture.harness.token)
        .await;
    match response.status {
        StatusCode::OK => assert_eq!(
            response.bytes(),
            CLIP,
            "a case-variant name resolved to something other than this owner's artifact"
        ),
        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST => {}
        status => panic!("a case-variant artifact name answered {status}"),
    }
    assert!(
        !response.bytes().windows(SECRET.len()).any(|w| w == SECRET),
        "a case-variant name leaked content from above the artifact root"
    );
}

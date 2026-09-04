//! Durable generated-image persistence beneath a protected artifact root.

use std::path::{Component, Path, PathBuf};
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("invalid artifact path")]
    InvalidPath,
    #[error("artifact I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn persist(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        owner_id: Uuid,
        extension: &str,
        bytes: &[u8],
    ) -> Result<String, ArtifactError> {
        let extension = safe_extension(extension)?;
        let artifact_id = Uuid::new_v4();
        let filename = format!("{artifact_id}.{extension}");
        let directory = self
            .root
            .join(workspace_id.to_string())
            .join(chat_id.to_string())
            .join(owner_id.to_string());
        fs::create_dir_all(&directory).await?;
        fs::write(directory.join(&filename), bytes).await?;
        Ok(format!(
            "/api/artifacts/{workspace_id}/{chat_id}/{owner_id}/{filename}"
        ))
    }

    pub async fn read(
        &self,
        workspace_id: Uuid,
        chat_id: Uuid,
        owner_id: Uuid,
        filename: &str,
    ) -> Result<Vec<u8>, ArtifactError> {
        if !safe_filename(filename) {
            return Err(ArtifactError::InvalidPath);
        }
        let candidate = self
            .root
            .join(workspace_id.to_string())
            .join(chat_id.to_string())
            .join(owner_id.to_string())
            .join(filename);
        ensure_lexically_beneath(&self.root, &candidate)?;
        let root = fs::canonicalize(&self.root).await?;
        let canonical = fs::canonicalize(candidate).await?;
        if !canonical.starts_with(root) {
            return Err(ArtifactError::InvalidPath);
        }
        Ok(fs::read(canonical).await?)
    }

    pub async fn cleanup_chat(&self, workspace_id: Uuid, chat_id: Uuid) {
        let path = self
            .root
            .join(workspace_id.to_string())
            .join(chat_id.to_string());
        if let Err(error) = fs::remove_dir_all(path).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "Failed to clean up artifacts for chat {}: {}",
                chat_id,
                error
            );
        }
    }

    pub async fn cleanup_owner(&self, workspace_id: Uuid, chat_id: Uuid, owner_id: Uuid) {
        let path = self
            .root
            .join(workspace_id.to_string())
            .join(chat_id.to_string())
            .join(owner_id.to_string());
        if let Err(error) = fs::remove_dir_all(path).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "Failed to clean up artifacts for message {}: {}",
                owner_id,
                error
            );
        }
    }
}

fn safe_extension(extension: &str) -> Result<&str, ArtifactError> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Ok("png"),
        "jpg" | "jpeg" => Ok("jpg"),
        "webp" => Ok("webp"),
        _ => Err(ArtifactError::InvalidPath),
    }
}

fn safe_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 128
        && Path::new(filename)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && filename
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

fn ensure_lexically_beneath(root: &Path, candidate: &Path) -> Result<(), ArtifactError> {
    if candidate.starts_with(root)
        && candidate.strip_prefix(root).is_ok_and(|relative| {
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        })
    {
        Ok(())
    } else {
        Err(ArtifactError::InvalidPath)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_and_unsafe_extensions_are_rejected() {
        assert!(!safe_filename("../secret.png"));
        assert!(!safe_filename("nested/file.png"));
        assert!(!safe_filename("%2e%2e.png"));
        assert!(safe_filename("4f20_image-1.png"));
        assert!(safe_extension("../png").is_err());
        assert!(safe_extension("svg").is_err());
    }

    #[test]
    fn candidate_must_remain_beneath_root() {
        assert!(ensure_lexically_beneath(Path::new("/tmp/a"), Path::new("/tmp/a/x/y")).is_ok());
        assert!(ensure_lexically_beneath(Path::new("/tmp/a"), Path::new("/tmp/b/y")).is_err());
    }

    #[tokio::test]
    async fn persists_reads_and_cleans_owner_directory() {
        let root = std::env::temp_dir().join(format!("zone-artifacts-{}", Uuid::new_v4()));
        let store = ArtifactStore::new(root.clone());
        let workspace = Uuid::new_v4();
        let chat = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let url = store
            .persist(workspace, chat, owner, "png", b"png-data")
            .await
            .unwrap();
        let filename = url.rsplit('/').next().unwrap();
        assert_eq!(
            store.read(workspace, chat, owner, filename).await.unwrap(),
            b"png-data"
        );
        assert!(matches!(
            store.read(workspace, chat, owner, "../secret").await,
            Err(ArtifactError::InvalidPath)
        ));
        store.cleanup_owner(workspace, chat, owner).await;
        assert!(
            !root
                .join(workspace.to_string())
                .join(chat.to_string())
                .join(owner.to_string())
                .exists()
        );
        let _ = fs::remove_dir_all(root).await;
    }
}

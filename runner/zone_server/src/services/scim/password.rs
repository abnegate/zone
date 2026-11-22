use super::failure::Failure;
use crate::auth::hash_password;
use crate::utils::crypto::generate_token;

const HASHING_FAILED: &str = "Could not hash a provisioned account's password";

pub async fn unusable() -> Result<String, Failure> {
    tokio::task::spawn_blocking(|| hash_password(&generate_token()))
        .await
        .map_err(|_| Failure::Internal(HASHING_FAILED))?
        .map_err(|_| Failure::Internal(HASHING_FAILED))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::password::verify_password;

    #[tokio::test]
    async fn each_unusable_password_is_a_fresh_argon2_hash() {
        let first = unusable().await.unwrap();
        let second = unusable().await.unwrap();
        assert!(first.starts_with("$argon2"), "{first}");
        assert_ne!(first, second);
        assert!(!verify_password("", &first).unwrap());
    }
}

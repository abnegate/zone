use std::future::Future;

use abnegate_secret::SecretValue;
use tokio::sync::OnceCell;

use super::error::Error;

#[derive(Default)]
pub(super) struct Mint {
    minted: OnceCell<Result<SecretValue, Error>>,
    confirmed: OnceCell<Error>,
}

impl Mint {
    pub(super) async fn minted<F, Minting>(&self, mint: F) -> Result<SecretValue, Error>
    where
        F: FnOnce() -> Minting,
        Minting: Future<Output = Result<SecretValue, Error>>,
    {
        self.minted
            .get_or_init(mint)
            .await
            .as_ref()
            .cloned()
            .map_err(Error::duplicate)
    }

    pub(super) async fn confirmed<F, Confirming>(&self, confirm: F) -> Error
    where
        F: FnOnce() -> Confirming,
        Confirming: Future<Output = Error>,
    {
        self.confirmed.get_or_init(confirm).await.duplicate()
    }
}

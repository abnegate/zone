//! Why an installation GitHub described was not filed, worded so none can
//! carry a credential.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Unrecorded {
    #[error("The installation belongs to a different GitHub App.")]
    ForeignApplication,
    #[error("GitHub numbered the installation beyond what Zone can store.")]
    OutOfRange,
    #[error("GitHub did not give an http(s) address for the installation.")]
    Unaddressable,
    #[error("GitHub installed the App on a kind of account Zone does not know.")]
    UnknownAccountType,
    #[error("GitHub reported a repository selection Zone does not know.")]
    UnknownRepositorySelection,
    #[error("The installation is already connected to another GitHub App, or the App is gone.")]
    Foreign,
    #[error("The installation could not be stored: {0}")]
    Database(#[source] sqlx::Error),
}

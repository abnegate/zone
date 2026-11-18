use clap::Subcommand;

use crate::keygen_arguments::KeygenArguments;
use crate::sign_arguments::SignArguments;

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Generate a new signing key pair.
    Keygen(KeygenArguments),
    /// Sign a license with a key the production keyring trusts.
    Sign(SignArguments),
}

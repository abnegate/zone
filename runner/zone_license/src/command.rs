use clap::Subcommand;

use crate::keygen_arguments::KeygenArguments;
use crate::registry_command::RegistryCommand;
use crate::sign_arguments::SignArguments;

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Generate a new signing key pair.
    Keygen(KeygenArguments),
    /// Sign a license with a key the production keyring trusts.
    Sign(SignArguments),
    /// Manage the keys the container registry verifies pull tokens with.
    #[command(subcommand)]
    Registry(RegistryCommand),
    /// Serve the Polar webhook, emailing a signed license for each paid order.
    ///
    /// Every setting comes from the environment; see docs/LICENSING.md.
    Serve,
}

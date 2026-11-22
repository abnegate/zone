use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct RegistryArguments {
    /// A token key generation to publish, from 1. Repeat it to publish several during a rotation.
    #[arg(long = "generation", default_values_t = [NonZeroU32::MIN])]
    pub generations: Vec<NonZeroU32>,
    /// Defaults to ZONE_LICENSE_KEY_ID.
    #[arg(long)]
    pub key_id: Option<String>,
    /// Defaults to ZONE_LICENSE_PRIVATE_KEY_FILE, or the key in ZONE_LICENSE_PRIVATE_KEY.
    #[arg(long)]
    pub private_key_file: Option<PathBuf>,
}

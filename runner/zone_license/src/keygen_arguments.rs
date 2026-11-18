use std::path::PathBuf;

use clap::Args;

#[derive(Debug, Clone, Default, Args)]
pub struct KeygenArguments {
    /// Write the private key to this new file, created with mode 600, instead of stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// The key id to print, instead of the next free one for this year, such as 2026-2.
    #[arg(long)]
    pub key_id: Option<String>,
}

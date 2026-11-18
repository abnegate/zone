use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::Args;
use uuid::Uuid;

use crate::instant;

#[derive(Debug, Clone, Args)]
pub struct SignArguments {
    /// The licensee's name, as the console shows it.
    #[arg(long)]
    pub customer: String,
    #[arg(long)]
    pub seats: u32,
    /// An RFC 3339 timestamp, or a YYYY-MM-DD date meaning midnight UTC.
    #[arg(long, value_parser = instant::parse)]
    pub expires_at: DateTime<Utc>,
    /// Defaults to ZONE_LICENSE_KEY_ID.
    #[arg(long)]
    pub key_id: Option<String>,
    /// Defaults to ZONE_LICENSE_PRIVATE_KEY_FILE, or the key in ZONE_LICENSE_PRIVATE_KEY.
    #[arg(long)]
    pub private_key_file: Option<PathBuf>,
    /// Defaults to now. Accepts the same forms as --expires-at.
    #[arg(long, value_parser = instant::parse)]
    pub issued_at: Option<DateTime<Utc>>,
    /// Defaults to a random UUID.
    #[arg(long)]
    pub id: Option<Uuid>,
}

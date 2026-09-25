//! What a console hands back to finish a Claude sign-in claude.com sent to the callback.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ReceiptRequest {
    /// What the callback listener sent the browser on to the console with.
    pub receipt: String,
}

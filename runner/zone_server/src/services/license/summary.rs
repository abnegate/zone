use serde_json::{Value, json};
use zone_enterprise::License;

/// What the audit log keeps of a license. Never the key itself: whoever can
/// read the audit log could otherwise lift it onto another server.
pub fn summary(license: &License) -> Value {
    json!({
        "id": license.id(),
        "plan": license.plan(),
        "customer": license.customer(),
        "seats": license.seats(),
        "expires_at": license.expires_at(),
        "key_id": license.key_id(),
    })
}

#[cfg(test)]
mod tests;

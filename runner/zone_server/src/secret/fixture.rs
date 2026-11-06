use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;

const V1_ENVELOPE_OPENING: &str = "ENC[v1:";
const ENVELOPE_CLOSING: &str = "]";

/// `credential` sealed in an envelope under `key`.
pub fn seal(credential: &str, key: &MasterKey) -> String {
    encrypt_value(&SecretValue::new(credential), key).expect("a credential seals")
}

/// `credential` sealed under `key` the way Zone stored it before envelopes existed: the bare
/// body an envelope wraps.
pub fn seal_legacy(credential: &str, key: &MasterKey) -> String {
    seal(credential, key)
        .strip_prefix(V1_ENVELOPE_OPENING)
        .and_then(|sealed| sealed.strip_suffix(ENVELOPE_CLOSING))
        .expect("a v1 envelope")
        .to_string()
}

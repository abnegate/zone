use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;

const ENVELOPE_PREFIX: &str = "ENC[v1:";
const ENVELOPE_SUFFIX: &str = "]";

/// `credential` sealed in an envelope under `key`.
pub fn seal(credential: &str, key: &MasterKey) -> String {
    encrypt_value(&SecretValue::new(credential), key).expect("a credential seals")
}

/// `credential` sealed under `key` the way Zone stored it before envelopes existed: the bare
/// body an envelope wraps.
pub fn seal_legacy(credential: &str, key: &MasterKey) -> String {
    seal(credential, key)
        .strip_prefix(ENVELOPE_PREFIX)
        .and_then(|sealed| sealed.strip_suffix(ENVELOPE_SUFFIX))
        .expect("a v1 envelope")
        .to_string()
}

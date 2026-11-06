use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;

/// The passphrase [`PREVIOUS_WRITER_SEALED`] was sealed under.
pub const PREVIOUS_WRITER_PASSPHRASE: &str = "a-high-entropy-passphrase-of-at-least-32-bytes";

/// The credential [`PREVIOUS_WRITER_SEALED`] seals.
pub const PREVIOUS_WRITER_CREDENTIAL: &str = "ghp_sealed_by_the_previous_writer";

/// [`PREVIOUS_WRITER_CREDENTIAL`] as Zone's own `crypto::encrypt` sealed it before the switch to
/// `abnegate-secret`, under the key [`PREVIOUS_WRITER_PASSPHRASE`] derives.
pub const PREVIOUS_WRITER_SEALED: &str =
    "/MH1Kaa9KYTm3i5U3+msvMzJTrL/7hIYTHvl1iie3MMT+e2ZcaO81x6LZY5Af9PjFmg/1N1Vv8dOI+QQ4A==";

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

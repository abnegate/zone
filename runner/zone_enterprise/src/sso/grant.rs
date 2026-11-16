#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant<'a> {
    pub code: &'a str,
    pub verifier: &'a str,
    pub nonce: &'a str,
    pub redirect_uri: &'a str,
}

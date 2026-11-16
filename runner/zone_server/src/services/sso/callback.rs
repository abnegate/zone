/// What the identity provider sent back to the console's callback page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Callback<'a> {
    pub state: &'a str,
    pub code: &'a str,
}

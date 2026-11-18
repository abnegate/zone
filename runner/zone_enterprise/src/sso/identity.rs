#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub subject: String,
    pub email: String,
    pub name: Option<String>,
}

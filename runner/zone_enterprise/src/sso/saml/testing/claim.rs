#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub name: String,
    pub values: Vec<String>,
}

impl Claim {
    pub fn new(name: &str, values: &[&str]) -> Self {
        Self {
            name: name.to_owned(),
            values: values.iter().map(|value| (*value).to_owned()).collect(),
        }
    }
}

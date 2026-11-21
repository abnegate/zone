use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Customer {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub billing_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_may_be_absent() {
        let customer: Customer = serde_json::from_str(r#"{"name":null}"#).unwrap();
        assert_eq!(customer.email, None);
        assert_eq!(customer.name, None);
        assert_eq!(customer.billing_name, None);
    }
}

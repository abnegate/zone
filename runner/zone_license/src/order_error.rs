use crate::variable::Variable;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrderError {
    #[error("The Polar event is not in the expected shape.")]
    Malformed,
    #[error("The Polar order has no customer email to send the license to.")]
    Email { order: String },
    #[error(
        "The Polar order is for a product in {} but has no seat-based subscription. Check the product's pricing.",
        Variable::Products.name()
    )]
    Misconfigured { order: String, product: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_no_order_details() {
        assert_eq!(
            OrderError::Malformed.to_string(),
            "The Polar event is not in the expected shape."
        );
        assert_eq!(
            OrderError::Email {
                order: "order".into()
            }
            .to_string(),
            "The Polar order has no customer email to send the license to."
        );
        assert_eq!(
            OrderError::Misconfigured {
                order: "order".into(),
                product: "product".into()
            }
            .to_string(),
            "The Polar order is for a product in POLAR_PRODUCT_IDS but has no seat-based subscription. Check the product's pricing."
        );
    }
}

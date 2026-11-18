#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OrderError {
    #[error("The Polar event is not in the expected shape.")]
    Malformed,
    #[error("The Polar order has no customer email to send the license to.")]
    Email,
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
            OrderError::Email.to_string(),
            "The Polar order has no customer email to send the license to."
        );
    }
}

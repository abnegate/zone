#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Could not read the audit events to export: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Could not encode an audit event for export: {0}")]
    Encode(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_failure_names_its_cause() {
        assert_eq!(
            ExportError::from(sqlx::Error::PoolClosed).to_string(),
            format!(
                "Could not read the audit events to export: {}",
                sqlx::Error::PoolClosed
            )
        );
        let encoding = serde_json::from_str::<u8>("x").unwrap_err();
        let shown = encoding.to_string();
        assert_eq!(
            ExportError::from(encoding).to_string(),
            format!("Could not encode an audit event for export: {shown}")
        );
    }
}

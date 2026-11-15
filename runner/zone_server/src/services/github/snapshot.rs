use std::sync::Arc;

use chrono::DateTime;
use chrono::Utc;

#[derive(Debug)]
pub struct Snapshot<T> {
    pub value: Arc<T>,
    pub generated_at: DateTime<Utc>,
    pub warning: Option<String>,
}

impl<T> Snapshot<T> {
    pub fn new(value: T, generated_at: DateTime<Utc>) -> Self {
        Self {
            value: Arc::new(value),
            generated_at,
            warning: None,
        }
    }

    pub fn warned(&self, warning: Option<String>) -> Self {
        Self {
            value: Arc::clone(&self.value),
            generated_at: self.generated_at,
            warning,
        }
    }
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        self.warned(self.warning.clone())
    }
}

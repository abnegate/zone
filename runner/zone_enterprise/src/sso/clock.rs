use std::sync::Arc;

use chrono::{DateTime, Utc};

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

pub fn system() -> Clock {
    Arc::new(Utc::now)
}

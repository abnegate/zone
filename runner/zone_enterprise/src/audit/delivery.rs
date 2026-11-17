use std::time::Duration;

pub const RETRY_BASE: Duration = Duration::from_secs(5);
pub const RETRY_FACTOR: u32 = 2;
pub const RETRY_MAXIMUM: Duration = Duration::from_secs(15 * 60);
pub const S3_FLUSH_AGE: Duration = Duration::from_secs(60);
pub const STALL_AGE: Duration = Duration::from_secs(5 * 60);
pub const WEBHOOK_BATCH: usize = 100;
pub const S3_BATCH: usize = 1000;
pub const SYSLOG_BATCH: usize = 500;

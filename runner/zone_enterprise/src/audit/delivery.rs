use std::time::Duration;

use chrono::TimeDelta;

pub const RETRY_BASE: Duration = Duration::from_secs(5);
pub const RETRY_FACTOR: u32 = 2;
pub const RETRY_MAXIMUM: Duration = Duration::from_secs(15 * 60);
pub const S3_FLUSH_AGE: TimeDelta = TimeDelta::seconds(60);
pub const STALL_AGE: TimeDelta = TimeDelta::minutes(5);
pub const WEBHOOK_BATCH: usize = 100;
pub const S3_BATCH: usize = 1000;
pub const SYSLOG_BATCH: usize = 500;

const _: () = {
    assert!(!RETRY_BASE.is_zero());
    assert!(RETRY_FACTOR > 1);
    assert!(RETRY_BASE.as_nanos() < RETRY_MAXIMUM.as_nanos());
    assert!(WEBHOOK_BATCH > 0 && S3_BATCH > 0 && SYSLOG_BATCH > 0);
    assert!(S3_FLUSH_AGE.num_seconds() > 0 && STALL_AGE.num_seconds() > 0);
};

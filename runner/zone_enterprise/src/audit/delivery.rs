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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_from_five_seconds_doubling_to_fifteen_minutes() {
        assert_eq!(RETRY_BASE, Duration::from_secs(5));
        assert_eq!(RETRY_FACTOR, 2);
        assert_eq!(RETRY_MAXIMUM, Duration::from_secs(900));
    }

    #[test]
    fn ages_are_a_minute_for_s3_and_five_minutes_for_a_stall() {
        assert_eq!(S3_FLUSH_AGE.num_seconds(), 60);
        assert_eq!(STALL_AGE.num_seconds(), 300);
    }

    #[test]
    fn batch_sizes_match_each_transport() {
        assert_eq!(WEBHOOK_BATCH, 100);
        assert_eq!(S3_BATCH, 1000);
        assert_eq!(SYSLOG_BATCH, 500);
    }
}

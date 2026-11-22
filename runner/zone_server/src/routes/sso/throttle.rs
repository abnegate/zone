#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn a_retry_waits_until_the_window_resets_rounded_up() {
        let now = Instant::now();

        assert_eq!(retry_after(Some(now + Duration::from_millis(12_300)), now), 13);
        assert_eq!(retry_after(Some(now + Duration::from_secs(45)), now), 45);
        assert_eq!(retry_after(Some(now + Duration::from_millis(200)), now), 1);
    }

    #[test]
    fn a_window_with_no_reset_or_one_already_past_waits_a_whole_window() {
        let now = Instant::now();

        assert_eq!(retry_after(None, now), 60);
        assert_eq!(retry_after(Some(now), now), 60);
        assert_eq!(
            retry_after(now.checked_sub(Duration::from_secs(1)), now),
            60
        );
    }

    #[test]
    fn the_refusal_explains_itself_in_one_sentence_pair() {
        assert_eq!(
            RATE_LIMITED_MESSAGE,
            "Too many sign-in attempts from your network. Wait a minute, then try again."
        );
    }
}

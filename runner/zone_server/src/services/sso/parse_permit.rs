#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Semaphore;
    use tokio::time::Instant;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_permit_freed_within_the_wait_is_taken() {
        let semaphore = Arc::new(Semaphore::new(1));
        let held = Arc::clone(&semaphore).acquire_owned().await.unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(held);
        });
        let started = Instant::now();

        let permit = acquire(&semaphore, PARSE_PERMIT_WAIT)
            .await
            .expect("the permit freed after 100 ms");

        assert_eq!(started.elapsed(), Duration::from_millis(100));
        assert_eq!(semaphore.available_permits(), 0);
        drop(permit);
        assert_eq!(semaphore.available_permits(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_permit_still_held_after_the_wait_is_overloaded() {
        let semaphore = Arc::new(Semaphore::new(1));
        let _held = Arc::clone(&semaphore).acquire_owned().await.unwrap();
        let started = Instant::now();

        let refused = acquire(&semaphore, PARSE_PERMIT_WAIT).await;

        assert!(matches!(refused, Err(SignInFailure::Overloaded)));
        assert_eq!(started.elapsed(), PARSE_PERMIT_WAIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_free_permit_is_taken_without_waiting() {
        let semaphore = Arc::new(Semaphore::new(1));
        let started = Instant::now();

        let _permit = acquire(&semaphore, PARSE_PERMIT_WAIT).await.unwrap();

        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test]
    async fn a_closed_semaphore_is_overloaded() {
        let semaphore = Arc::new(Semaphore::new(1));
        semaphore.close();

        assert!(matches!(
            acquire(&semaphore, PARSE_PERMIT_WAIT).await,
            Err(SignInFailure::Overloaded)
        ));
    }

    #[test]
    fn a_post_waits_two_seconds_for_a_permit() {
        assert_eq!(PARSE_PERMIT_WAIT, Duration::from_secs(2));
    }
}

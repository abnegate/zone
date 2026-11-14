use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;

use super::Cache;
use super::IDLE_EXPIRY;
use super::RATE_LIMIT_BACKOFF;
use crate::services::github::client::RATE_LIMIT_EXHAUSTED;
use crate::services::github::client::TOO_MANY_REQUESTS;
use crate::services::github::client::status_error;
use crate::services::github::fault::Fault;
use crate::services::github::freshness::Freshness;

const KEY: u8 = 1;
const OTHER: u8 = 2;
const TTL: Duration = Duration::from_secs(45);
const FLOOR: Duration = Duration::from_secs(10);
const UNREACHABLE: &str = "GitHub request failed or timed out.";

fn cache() -> Cache<u8, u32> {
    Cache::new(TTL, FLOOR)
}

fn later(start: DateTime<Utc>, seconds: i64) -> DateTime<Utc> {
    start + TimeDelta::seconds(seconds)
}

fn unreachable() -> Fault {
    Fault::upstream(UNREACHABLE)
}

fn exhausted() -> Fault {
    Fault::RateLimited {
        retry_after: None,
        exhausted: true,
    }
}

async fn never(_: Option<Arc<u32>>) -> Result<u32, Fault> {
    panic!("the cache must answer without asking GitHub")
}

async fn seeded(cache: &Cache<u8, u32>, key: u8, at: DateTime<Utc>, value: u32) {
    let (snapshot, freshness) = cache
        .read(key, false, at, |_| async move { Ok(value) })
        .await
        .expect("the first read fetches");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, value);
    assert_eq!(snapshot.generated_at, at);
    assert!(snapshot.warning.is_none());
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_younger_than_its_ttl_is_served_without_asking_again() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 44), never)
        .await
        .expect("a fresh snapshot is served");

    assert_eq!(freshness, Freshness::Cached);
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.generated_at, start);
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_is_stamped_when_its_fetch_settles_not_when_it_starts() {
    let cache = cache();
    let start = Utc::now();

    let (snapshot, _) = cache
        .read(KEY, false, start, |_| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(7)
        })
        .await
        .expect("a slow fetch still answers");

    assert_eq!(snapshot.generated_at, later(start, 30));
    let (_, freshness) = cache
        .read(KEY, false, later(start, 60), never)
        .await
        .expect("the ttl runs from when the fetch settled");
    assert_eq!(freshness, Freshness::Cached);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_inside_the_floor_is_served_the_snapshot_it_already_has() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let (snapshot, freshness) = cache
        .read(KEY, true, later(start, 9), never)
        .await
        .expect("a floored refresh is served");

    assert_eq!(freshness, Freshness::Floored);
    assert_eq!(*snapshot.value, 7);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_past_the_floor_asks_again_with_what_it_had() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let now = later(start, 10);

    let (snapshot, freshness) = cache
        .read(KEY, true, now, |previous| async move {
            assert_eq!(previous.as_deref(), Some(&7));
            Ok(8)
        })
        .await
        .expect("a refresh past the floor fetches");

    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
    assert_eq!(snapshot.generated_at, now);
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_past_its_ttl_is_fetched_again_with_what_it_had() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 45), |previous| async move {
            Ok(previous.map_or(0, |value| *value + 1))
        })
        .await
        .expect("an expired snapshot is fetched again");

    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
}

#[tokio::test(start_paused = true)]
async fn a_failed_fetch_serves_the_previous_snapshot_as_stale_with_why() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let (snapshot, freshness) = cache
        .read(KEY, true, later(start, 60), |_| async {
            Err(unreachable())
        })
        .await
        .expect("a previous snapshot outlives a failed fetch");

    assert_eq!(freshness, Freshness::Stale);
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.generated_at, start);
    assert_eq!(snapshot.warning.as_deref(), Some(UNREACHABLE));
    assert_eq!(
        cache.peek(&KEY).and_then(|snapshot| snapshot.warning),
        Some(UNREACHABLE.to_string())
    );

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 61), |_| async { Ok(9) })
        .await
        .expect("an ordinary failure does not hold the next read back");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 9);
    assert!(snapshot.warning.is_none());
    assert!(
        cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_none())
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_fetch_with_nothing_before_it_is_an_error_and_leaves_no_slot() {
    let cache = cache();

    let fault = cache
        .read(KEY, false, Utc::now(), |_| async { Err(unreachable()) })
        .await
        .expect_err("there is nothing to serve");

    assert_eq!(fault, unreachable());
    assert!(cache.peek(&KEY).is_none());
    assert!(!cache.holds(&KEY), "a key that never answered is not kept");
}

#[tokio::test(start_paused = true)]
async fn an_exhausted_rate_limit_holds_every_read_back_for_the_backoff() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let limited = later(start, 60);

    let (snapshot, freshness) = cache
        .read(KEY, false, limited, |_| async { Err(exhausted()) })
        .await
        .expect("the previous snapshot is served");
    assert_eq!(freshness, Freshness::Stale);
    assert_eq!(snapshot.warning.as_deref(), Some(RATE_LIMIT_EXHAUSTED));

    let backoff = TimeDelta::from_std(RATE_LIMIT_BACKOFF).unwrap();
    for refresh in [false, true] {
        let (snapshot, freshness) = cache
            .read(
                KEY,
                refresh,
                limited + backoff - TimeDelta::seconds(1),
                never,
            )
            .await
            .expect("the stale snapshot is served inside the backoff");
        assert_eq!(freshness, Freshness::Stale, "refresh {refresh}");
        assert_eq!(*snapshot.value, 7);
        assert_eq!(snapshot.warning.as_deref(), Some(RATE_LIMIT_EXHAUSTED));
    }

    let (snapshot, freshness) = cache
        .read(KEY, false, limited + backoff, |_| async { Ok(8) })
        .await
        .expect("the backoff ends");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
}

#[tokio::test(start_paused = true)]
async fn a_secondary_rate_limit_backs_off_like_an_exhausted_one() {
    let cache = cache();
    let start = Utc::now();
    let limited = Fault::RateLimited {
        retry_after: None,
        exhausted: false,
    };

    let fault = cache
        .read(KEY, false, start, |_| async move { Err(limited) })
        .await
        .expect_err("there is nothing to serve");
    assert_eq!(fault.to_string(), status_error(TOO_MANY_REQUESTS));

    let fault = cache
        .read(KEY, true, later(start, 299), never)
        .await
        .expect_err("the backoff refuses without asking");
    assert_eq!(fault.to_string(), status_error(TOO_MANY_REQUESTS));
    assert!(cache.holds(&KEY), "a key backing off keeps its slot");

    let (snapshot, _) = cache
        .read(KEY, false, later(start, 300), |_| async { Ok(7) })
        .await
        .expect("the backoff ends");
    assert_eq!(*snapshot.value, 7);
}

#[tokio::test(start_paused = true)]
async fn a_rate_limit_that_names_its_reset_backs_off_until_then() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let limited = later(start, 60);

    cache
        .read(KEY, false, limited, |_| async {
            Err(Fault::RateLimited {
                retry_after: Some(Duration::from_secs(30)),
                exhausted: true,
            })
        })
        .await
        .expect("the previous snapshot is served");

    let (_, freshness) = cache
        .read(KEY, true, later(start, 89), never)
        .await
        .expect("held back until the reset");
    assert_eq!(freshness, Freshness::Stale);
    let (snapshot, freshness) = cache
        .read(KEY, true, later(start, 90), |_| async { Ok(8) })
        .await
        .expect("asked again once the limit resets");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
}

#[tokio::test(start_paused = true)]
async fn an_exhausted_rate_limit_with_nothing_before_it_refuses_without_asking() {
    let cache = cache();
    let start = Utc::now();

    let fault = cache
        .read(KEY, false, start, |_| async { Err(exhausted()) })
        .await
        .expect_err("there is nothing to serve");
    assert_eq!(fault, exhausted());

    let fault = cache
        .read(KEY, true, later(start, 299), never)
        .await
        .expect_err("the backoff refuses without asking");
    assert_eq!(fault.to_string(), RATE_LIMIT_EXHAUSTED);
}

#[tokio::test(start_paused = true)]
async fn a_slot_nobody_reads_for_an_hour_is_dropped() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let idle = TimeDelta::from_std(IDLE_EXPIRY).unwrap();

    seeded(&cache, OTHER, start + idle - TimeDelta::seconds(1), 70).await;
    assert!(cache.holds(&KEY), "an hour has not passed");

    seeded(&cache, OTHER, start + idle + TimeDelta::seconds(59), 71).await;
    assert!(!cache.holds(&KEY), "an hour without a read drops the slot");
    assert!(cache.holds(&OTHER));
}

#[tokio::test(start_paused = true)]
async fn concurrent_reads_of_one_key_ask_github_once() {
    let cache = cache();
    let now = Utc::now();
    let fetches = AtomicUsize::new(0);
    let counted = &fetches;
    let fetch = move |_: Option<Arc<u32>>| async move {
        counted.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        Ok(7)
    };

    let (first, second) = tokio::join!(
        cache.read(KEY, false, now, fetch),
        cache.read(KEY, false, now, fetch)
    );

    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    let (first, first_freshness) = first.expect("the first read fetches");
    let (second, second_freshness) = second.expect("the second read waits for it");
    assert_eq!(first_freshness, Freshness::Refreshed);
    assert_eq!(second_freshness, Freshness::Cached);
    assert!(Arc::ptr_eq(&first.value, &second.value));
}

#[tokio::test(start_paused = true)]
async fn an_invalidated_snapshot_is_fetched_again_with_what_it_had() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    seeded(&cache, OTHER, start, 70).await;

    cache.invalidate(&KEY);

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 1), |previous| async move {
            assert_eq!(previous.as_deref(), Some(&7));
            Ok(8)
        })
        .await
        .expect("an invalidated snapshot is fetched");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
    let (snapshot, freshness) = cache
        .read(OTHER, false, later(start, 1), never)
        .await
        .expect("another key keeps its snapshot");
    assert_eq!(freshness, Freshness::Cached);
    assert_eq!(*snapshot.value, 70);
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_invalidated_while_it_was_fetched_is_fetched_again() {
    let cache = cache();
    let start = Utc::now();
    let writer = &cache;

    let (snapshot, _) = cache
        .read(KEY, false, start, move |_| async move {
            writer.invalidate(&KEY);
            Ok(7)
        })
        .await
        .expect("the read that raced the write still answers");
    assert_eq!(*snapshot.value, 7);

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 1), |_| async { Ok(8) })
        .await
        .expect("what predates the write is not served as fresh");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
}

#[tokio::test(start_paused = true)]
async fn peeking_never_fetches() {
    let cache = cache();
    assert!(cache.peek(&KEY).is_none());

    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let snapshot = cache.peek(&KEY).expect("a stored snapshot is peeked");
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.generated_at, start);
    assert_eq!(cache.ttl(), TTL);
    assert_eq!(cache.floor(), FLOOR);
}

#[tokio::test(start_paused = true)]
async fn a_read_can_be_awaited_on_another_thread() {
    let cache = Arc::new(cache());
    let shared = Arc::clone(&cache);

    let (snapshot, freshness) = tokio::spawn(async move {
        shared
            .read(KEY, false, Utc::now(), |_| async { Ok(7) })
            .await
    })
    .await
    .expect("the read finishes")
    .expect("the read fetches");

    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 7);
    assert!(cache.peek(&KEY).is_some());
}

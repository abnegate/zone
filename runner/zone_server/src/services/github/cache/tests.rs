use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;

use chrono::DateTime;
use chrono::TimeDelta;
use chrono::Utc;
use futures::FutureExt;
use futures::future::BoxFuture;
use futures::poll;
use tokio::sync::Notify;

use super::Cache;
use super::IDLE_EXPIRY;
use super::RATE_LIMIT_BACKOFF;
use super::REFRESH_DEADLINE;
use super::REVALIDATE_WINDOW;
use crate::services::github::client::RATE_LIMIT_EXHAUSTED;
use crate::services::github::client::REQUEST_FAILED;
use crate::services::github::client::TOO_MANY_REQUESTS;
use crate::services::github::client::status_error;
use crate::services::github::fault::Fault;
use crate::services::github::freshness::Freshness;
use crate::services::github::snapshot::Snapshot;

const KEY: u8 = 1;
const OTHER: u8 = 2;
const TTL: Duration = Duration::from_secs(45);
const FLOOR: Duration = Duration::from_secs(10);
const UNREACHABLE: &str = "a GitHub stand-in that cannot be reached";
const YIELDS: usize = 64;

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

fn following(previous: Option<Arc<u32>>) -> Result<u32, Fault> {
    Ok(previous.map_or(0, |value| *value + 1))
}

fn counted<Answer>(
    fetches: &Arc<AtomicUsize>,
    answer: Answer,
) -> impl FnOnce(Option<Arc<u32>>) -> BoxFuture<'static, Result<u32, Fault>> + Send + 'static
where
    Answer: FnOnce(Option<Arc<u32>>) -> Result<u32, Fault> + Send + 'static,
{
    let fetches = Arc::clone(fetches);
    move |previous| {
        async move {
            fetches.fetch_add(1, Ordering::SeqCst);
            answer(previous)
        }
        .boxed()
    }
}

fn gated<Answer>(
    fetches: &Arc<AtomicUsize>,
    gate: &Arc<Notify>,
    answer: Answer,
) -> impl FnOnce(Option<Arc<u32>>) -> BoxFuture<'static, Result<u32, Fault>> + Send + 'static
where
    Answer: FnOnce(Option<Arc<u32>>) -> Result<u32, Fault> + Send + 'static,
{
    let (fetches, gate) = (Arc::clone(fetches), Arc::clone(gate));
    move |previous| {
        async move {
            fetches.fetch_add(1, Ordering::SeqCst);
            gate.notified().await;
            answer(previous)
        }
        .boxed()
    }
}

async fn at_once<Served: Future>(served: Served) -> Served::Output {
    let mut served = pin!(served);
    match poll!(served.as_mut()) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the cache answers without waiting for GitHub"),
    }
}

async fn eventually(settled: impl Fn() -> bool) -> bool {
    for _ in 0..YIELDS {
        if settled() {
            return true;
        }
        tokio::task::yield_now().await;
    }
    settled()
}

fn holding(cache: &Cache<u8, u32>, value: u32) -> bool {
    cache
        .peek(&KEY)
        .is_some_and(|snapshot| *snapshot.value == value)
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
async fn a_forgotten_snapshot_is_fetched_again_from_nothing() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    seeded(&cache, OTHER, start, 70).await;

    cache.forget(&KEY);

    assert!(cache.peek(&KEY).is_none(), "nothing forgotten is served");
    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 1), |previous| async move {
            assert_eq!(previous, None, "a forgotten snapshot is not reused");
            Ok(8)
        })
        .await
        .expect("a forgotten snapshot is fetched");
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
async fn a_snapshot_forgotten_while_it_was_fetched_is_not_reused() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let writer = &cache;

    let (snapshot, _) = cache
        .read(KEY, true, later(start, 60), move |_| async move {
            writer.forget(&KEY);
            Ok(8)
        })
        .await
        .expect("the read that raced the forget still answers");
    assert_eq!(*snapshot.value, 8);

    let (snapshot, freshness) = cache
        .read(KEY, false, later(start, 61), |previous| async move {
            assert_eq!(previous, None, "what predates the forget is not reused");
            Ok(9)
        })
        .await
        .expect("what predates the forget is fetched again");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 9);
}

async fn queued_behind_a_forgotten_fetch(
    cache: &Cache<u8, u32>,
    at: DateTime<Utc>,
    outcome: Result<u32, Fault>,
) -> Result<(Snapshot<u32>, Freshness), Fault> {
    let release = Notify::new();
    let (writer, released) = (cache, &release);
    let mut forgetting = pin!(cache.read(KEY, false, at, move |_| async move {
        writer.forget(&KEY);
        released.notified().await;
        Ok(8)
    }));
    let mut queued = pin!(cache.read(KEY, false, at, move |previous| async move {
        assert_eq!(previous, None, "what predates the forget is not reused");
        outcome
    }));

    assert!(
        poll!(forgetting.as_mut()).is_pending(),
        "the first read is fetching when it forgets"
    );
    assert!(
        poll!(queued.as_mut()).is_pending(),
        "the second read waits for the first fetch to settle"
    );
    release.notify_one();
    let (forgotten, queued) = tokio::join!(forgetting, queued);
    let (snapshot, _) = forgotten.expect("the read that raced the forget still answers");
    assert_eq!(*snapshot.value, 8);
    queued
}

#[tokio::test(start_paused = true)]
async fn a_fetch_queued_behind_a_forgotten_one_is_kept() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let raced = later(start, 60);

    let (snapshot, freshness) = queued_behind_a_forgotten_fetch(&cache, raced, Ok(9))
        .await
        .expect("the queued read fetches after the forget");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 9);

    let (snapshot, freshness) = cache
        .read(KEY, false, later(raced, 1), never)
        .await
        .expect("the queued fetch is served");
    assert_eq!(freshness, Freshness::Cached);
    assert_eq!(*snapshot.value, 9);
}

#[tokio::test(start_paused = true)]
async fn a_rate_limit_queued_behind_a_forgotten_fetch_is_kept() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let raced = later(start, 60);

    let fault = queued_behind_a_forgotten_fetch(&cache, raced, Err(exhausted()))
        .await
        .expect_err("the queued read is rate limited with nothing to serve");
    assert_eq!(fault, exhausted());

    let fault = cache
        .read(KEY, false, later(raced, 1), never)
        .await
        .expect_err("the backoff refuses without asking");
    assert_eq!(fault, exhausted());
}

#[tokio::test(start_paused = true)]
async fn forgetting_a_snapshot_keeps_its_rate_limit_backoff() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let limited = later(start, 60);
    cache
        .read(KEY, false, limited, |_| async { Err(exhausted()) })
        .await
        .expect("the previous snapshot is served");

    cache.forget(&KEY);

    let fault = cache
        .read(KEY, true, limited + TimeDelta::seconds(1), never)
        .await
        .expect_err("the backoff refuses without asking");
    assert_eq!(fault, exhausted());
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

#[tokio::test(start_paused = true)]
async fn a_snapshot_past_its_ttl_is_served_at_once_while_it_is_read_again() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));

    let served = at_once(cache.serve(
        KEY,
        false,
        later(start, 45),
        gated(&fetches, &gate, following),
    ))
    .await;

    let (snapshot, freshness) = served.expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.generated_at, start);
    assert!(snapshot.warning.is_none());
}

#[tokio::test(start_paused = true)]
async fn one_read_again_runs_however_many_readers_find_it_expired() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));

    for second in 45..=47 {
        let (snapshot, freshness) = at_once(cache.serve(
            KEY,
            false,
            later(start, second),
            gated(&fetches, &gate, following),
        ))
        .await
        .expect("the expired snapshot is served");
        assert_eq!(freshness, Freshness::Revalidating, "at {second}s");
        assert_eq!(*snapshot.value, 7, "at {second}s");
    }
    gate.notify_one();

    assert!(
        eventually(|| holding(&cache, 8)).await,
        "the read again settles what it fetched from what it had"
    );
    let (snapshot, freshness) = cache
        .serve(KEY, false, later(start, 48), counted(&fetches, following))
        .await
        .expect("the snapshot read again is served");
    assert_eq!(freshness, Freshness::Cached);
    assert_eq!(*snapshot.value, 8);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_past_the_floor_waits_for_github() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));

    let mut refreshed = pin!(cache.serve(
        KEY,
        true,
        later(start, 45),
        gated(&fetches, &gate, following)
    ));

    assert!(
        poll!(refreshed.as_mut()).is_pending(),
        "a refresh waits for GitHub"
    );
    gate.notify_one();
    let (snapshot, freshness) = refreshed.await.expect("the refresh fetches");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
}

#[tokio::test(start_paused = true)]
async fn a_key_never_read_or_invalidated_waits_for_github() {
    let cache = cache();
    let start = Utc::now();
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));

    for (at, expected) in [(start, 0), (later(start, 1), 1)] {
        let mut first = pin!(cache.serve(KEY, false, at, gated(&fetches, &gate, following)));
        assert!(
            poll!(first.as_mut()).is_pending(),
            "nothing current to serve, so the read waits for GitHub"
        );
        gate.notify_one();
        let (snapshot, freshness) = first.await.expect("the read fetches");
        assert_eq!(freshness, Freshness::Refreshed);
        assert_eq!(*snapshot.value, expected);
        cache.invalidate(&KEY);
    }
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_refresh_during_a_read_again_waits_for_it() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));
    let (_, freshness) = at_once(cache.serve(
        KEY,
        false,
        later(start, 45),
        gated(&fetches, &gate, following),
    ))
    .await
    .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);

    let mut refreshed =
        pin!(cache.serve(KEY, true, later(start, 46), counted(&fetches, following)));

    assert!(
        poll!(refreshed.as_mut()).is_pending(),
        "the refresh waits for the read already under way"
    );
    gate.notify_one();
    let (snapshot, freshness) = refreshed.await.expect("the refresh is answered");
    assert_eq!(freshness, Freshness::Floored);
    assert_eq!(*snapshot.value, 8);
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_failed_read_again_is_served_stale_with_why_and_tried_again() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let fetches = Arc::new(AtomicUsize::new(0));

    let (snapshot, freshness) = cache
        .serve(
            KEY,
            false,
            later(start, 45),
            counted(&fetches, |_| Err(unreachable())),
        )
        .await
        .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    assert_eq!(*snapshot.value, 7);
    assert!(
        eventually(|| cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_some()))
        .await,
        "the failed read again settles"
    );

    let (snapshot, freshness) = cache
        .serve(KEY, false, later(start, 46), counted(&fetches, following))
        .await
        .expect("the previous snapshot outlives a failed read again");
    assert_eq!(freshness, Freshness::Stale);
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.warning.as_deref(), Some(UNREACHABLE));
    assert_eq!(
        cache.peek(&KEY).and_then(|snapshot| snapshot.warning),
        Some(UNREACHABLE.to_string())
    );
    assert!(
        eventually(|| holding(&cache, 8)).await,
        "the next read again settles"
    );

    let (snapshot, freshness) = cache
        .serve(KEY, false, later(start, 47), counted(&fetches, following))
        .await
        .expect("the snapshot read again is served");
    assert_eq!(freshness, Freshness::Cached);
    assert_eq!(*snapshot.value, 8);
    assert!(snapshot.warning.is_none());
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_rate_limited_read_again_backs_off_like_an_inline_one() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let fetches = Arc::new(AtomicUsize::new(0));
    let limited = later(start, 45);

    let (_, freshness) =
        at_once(cache.serve(KEY, false, limited, counted(&fetches, |_| Err(exhausted()))))
            .await
            .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    assert!(
        eventually(|| cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_some()))
        .await,
        "the rate limited read again settles"
    );

    let backoff = TimeDelta::from_std(RATE_LIMIT_BACKOFF).unwrap();
    for refresh in [false, true] {
        let (snapshot, freshness) = cache
            .serve(
                KEY,
                refresh,
                limited + backoff - TimeDelta::seconds(1),
                counted(&fetches, following),
            )
            .await
            .expect("the stale snapshot is served inside the backoff");
        assert_eq!(freshness, Freshness::Stale, "refresh {refresh}");
        assert_eq!(*snapshot.value, 7);
        assert_eq!(snapshot.warning.as_deref(), Some(RATE_LIMIT_EXHAUSTED));
    }
    for _ in 0..YIELDS {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "GitHub is not asked again inside the backoff"
    );
}

async fn failed_read_again_is_served_stale(cache: &Cache<u8, u32>, at: DateTime<Utc>) {
    assert!(
        eventually(|| cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_some()))
        .await,
        "the failed read again settles"
    );
    let fetches = Arc::new(AtomicUsize::new(0));
    let (snapshot, freshness) = cache
        .serve(KEY, false, at, counted(&fetches, following))
        .await
        .expect("the previous snapshot outlives the failed read again");
    assert_eq!(freshness, Freshness::Stale);
    assert_eq!(*snapshot.value, 7);
    assert_eq!(snapshot.warning.as_deref(), Some(REQUEST_FAILED));
    assert!(
        eventually(|| holding(cache, 8)).await,
        "the next read again runs once the failed one let go"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_panicking_read_again_is_served_stale_with_why_and_frees_the_slot() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let fetches = Arc::new(AtomicUsize::new(0));

    let (_, freshness) = at_once(cache.serve(
        KEY,
        false,
        later(start, 45),
        counted(&fetches, |_| panic!("the read again panics")),
    ))
    .await
    .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    assert!(
        eventually(|| fetches.load(Ordering::SeqCst) == 1).await,
        "the panicking read again runs"
    );

    failed_read_again_is_served_stale(&cache, later(start, 46)).await;
}

#[tokio::test(start_paused = true)]
async fn a_read_again_past_its_deadline_is_served_stale_with_why_and_frees_the_slot() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;

    let (_, freshness) = at_once(cache.serve(KEY, false, later(start, 45), |_| {
        std::future::pending::<Result<u32, Fault>>()
    }))
    .await
    .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    for _ in 0..YIELDS {
        tokio::task::yield_now().await;
    }

    tokio::time::advance(REFRESH_DEADLINE - Duration::from_secs(1)).await;
    assert!(
        !eventually(|| cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_some()))
        .await,
        "a read again inside its deadline is still waited on"
    );
    tokio::time::advance(Duration::from_secs(1)).await;

    failed_read_again_is_served_stale(&cache, later(start, 46)).await;
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_past_the_revalidate_window_waits_for_github() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));
    let window = TimeDelta::from_std(TTL + REVALIDATE_WINDOW).unwrap();

    let (snapshot, freshness) = at_once(cache.serve(
        KEY,
        false,
        start + window - TimeDelta::seconds(1),
        counted(&fetches, |_| Err(unreachable())),
    ))
    .await
    .expect("a snapshot inside the window is served at once");
    assert_eq!(freshness, Freshness::Revalidating);
    assert_eq!(*snapshot.value, 7);
    assert!(
        eventually(|| cache
            .peek(&KEY)
            .is_some_and(|snapshot| snapshot.warning.is_some()))
        .await,
        "the failed read again settles"
    );

    let mut waiting = pin!(cache.serve(
        KEY,
        false,
        start + window,
        gated(&fetches, &gate, following)
    ));
    assert!(
        poll!(waiting.as_mut()).is_pending(),
        "a snapshot past the window is not served while it is read again"
    );
    gate.notify_one();
    let (snapshot, freshness) = waiting.await.expect("the read fetches");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 8);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn a_slot_forgotten_while_it_was_read_again_and_failing_is_dropped() {
    let cache = Arc::new(cache());
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let writer = Arc::clone(&cache);

    let (snapshot, freshness) = cache
        .serve(KEY, false, later(start, 45), move |_| async move {
            writer.forget(&KEY);
            Err(unreachable())
        })
        .await
        .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);
    assert_eq!(*snapshot.value, 7);

    assert!(
        eventually(|| !cache.holds(&KEY)).await,
        "a slot left with nothing to serve is dropped"
    );
}

#[tokio::test(start_paused = true)]
async fn a_snapshot_invalidated_while_it_was_read_again_is_fetched_again() {
    let cache = cache();
    let start = Utc::now();
    seeded(&cache, KEY, start, 7).await;
    let (fetches, gate) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));
    let (_, freshness) = at_once(cache.serve(
        KEY,
        false,
        later(start, 45),
        gated(&fetches, &gate, following),
    ))
    .await
    .expect("the expired snapshot is served");
    assert_eq!(freshness, Freshness::Revalidating);

    cache.invalidate(&KEY);
    let mut waiting = pin!(cache.serve(KEY, false, later(start, 46), counted(&fetches, |_| Ok(9))));

    assert!(
        poll!(waiting.as_mut()).is_pending(),
        "an invalidated snapshot waits for the read under way"
    );
    gate.notify_one();
    let (snapshot, freshness) = waiting
        .await
        .expect("what predates the write is fetched again");
    assert_eq!(freshness, Freshness::Refreshed);
    assert_eq!(*snapshot.value, 9);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

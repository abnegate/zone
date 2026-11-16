use std::time::Duration;

use tokio::time::Instant;

pub const EVENTUAL_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub async fn eventually<Check, Checked>(mut settled: Check) -> bool
where
    Check: FnMut() -> Checked,
    Checked: Future<Output = bool>,
{
    let deadline = Instant::now() + EVENTUAL_WAIT;
    while Instant::now() < deadline {
        if settled().await {
            return true;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    settled().await
}

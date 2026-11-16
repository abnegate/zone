use std::time::Duration;

pub const EVENTUAL_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub async fn eventually<Check, Checked>(mut settled: Check) -> bool
where
    Check: FnMut() -> Checked,
    Checked: Future<Output = bool>,
{
    let polled = async {
        while !settled().await {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    };
    tokio::time::timeout(EVENTUAL_WAIT, polled).await.is_ok()
}

//! The logins that lately refused a model for want of usage credits, which a session on that
//! model passes over while another login can run it.

use std::sync::LazyLock;

use chrono::{DateTime, TimeDelta, Utc};
use dashmap::DashMap;
use uuid::Uuid;

/// How long a login stays passed over for a model after it refused the model for want of usage
/// credits.
pub const COOL_DOWN: TimeDelta = TimeDelta::hours(1);

static REFUSALS: LazyLock<DashMap<(Uuid, String), DateTime<Utc>>> = LazyLock::new(DashMap::new);

/// Records that `login` refused `model` for want of usage credits at `at`, in place of any earlier
/// refusal of the same model.
pub fn record(login: Uuid, model: &str, at: DateTime<Utc>) {
    sweep();
    REFUSALS.insert((login, model.to_string()), at);
}

/// The logins that refused `model` for want of usage credits less than [`COOL_DOWN`] before `now`.
pub fn cooling(model: &str, now: DateTime<Utc>) -> Vec<Uuid> {
    sweep();
    REFUSALS
        .iter()
        .filter(|refusal| refusal.key().1 == model && cools(*refusal.value(), now))
        .map(|refusal| refusal.key().0)
        .collect()
}

fn cools(refused: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    refused + COOL_DOWN > now
}

fn sweep() {
    let now = Utc::now();
    REFUSALS.retain(|_, refused| cools(*refused, now));
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = "opus";

    #[test]
    fn a_refusal_cools_its_login_for_that_model_alone_until_the_cool_down_passes() {
        let now = Utc::now();
        let login = Uuid::new_v4();
        let other = Uuid::new_v4();

        record(login, MODEL, now);
        record(other, MODEL, now - COOL_DOWN);
        let refused = cooling(MODEL, now);
        let elsewhere = cooling("sonnet", now);
        let later = cooling(MODEL, now + COOL_DOWN);

        assert!(refused.contains(&login));
        assert!(!refused.contains(&other));
        assert!(
            !elsewhere.contains(&login),
            "a refusal is the model's alone"
        );
        assert!(!later.contains(&login));
        assert!(
            !REFUSALS.contains_key(&(other, MODEL.to_string())),
            "a refusal past its cool-down is swept"
        );
    }

    #[test]
    fn a_new_refusal_starts_the_cool_down_again() {
        let now = Utc::now();
        let login = Uuid::new_v4();

        record(login, MODEL, now - COOL_DOWN + TimeDelta::minutes(1));
        record(login, MODEL, now);

        assert!(cooling(MODEL, now + TimeDelta::minutes(30)).contains(&login));
    }
}

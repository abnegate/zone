use uuid::Uuid;
use zone_core::llm::Window;

use crate::services::login::router::Observed;
use crate::state::AppState;

/// The login an attempt runs under, and the usage windows the agent reported on it as the
/// attempt ran, the latest of each name, held until the attempt records them in one write.
pub(super) struct Meter<'a> {
    state: &'a AppState,
    login: Uuid,
    observed: Observed,
}

impl<'a> Meter<'a> {
    pub(super) fn new(state: &'a AppState, login: Uuid) -> Self {
        Self {
            state,
            login,
            observed: Observed::default(),
        }
    }

    /// Holds `window` over the one of the same name the attempt reported before.
    pub(super) fn observe(&mut self, window: Window) {
        self.observed.observe(self.login, window);
    }

    /// Records the windows held over the login's snapshot.
    pub(super) async fn record(&mut self) {
        self.observed.record(self.state, self.login).await;
    }
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;
    use zone_core::llm::AgentKind;

    use super::*;
    use crate::db::agent_logins::{self, Insert};

    fn window(name: &str, used_percent: f64) -> Window {
        Window {
            name: name.to_string(),
            used_percent: Some(used_percent),
            used: None,
            limit: None,
            resets_at: None,
        }
    }

    #[tokio::test]
    async fn an_attempt_writes_the_latest_of_each_window_once_when_it_records_them() {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Meter', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
        let login = agent_logins::insert(
            &pool,
            &Insert {
                organization_id: organization,
                agent: AgentKind::Claude.as_str(),
                account: Some("metered"),
                credential: None,
                label: Some("metered"),
                expires_at: None,
            },
        )
        .await
        .expect("a login")
        .id;
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let stored = async || {
            agent_logins::get(&pool, login)
                .await
                .expect("the login to be readable")
                .expect("the login")
                .snapshot()
        };
        let mut meter = Meter::new(&state, login);

        for used in 0..40 {
            meter.observe(window(Window::FIVE_HOURS, f64::from(used)));
        }
        meter.observe(window(Window::SEVEN_DAYS, 12.0));
        let running = stored().await;
        meter.record().await;
        let recorded = stored().await.expect("the recorded windows");
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        assert_eq!(
            running, None,
            "every window the agent reported was written as it came"
        );
        assert_eq!(
            recorded.windows,
            [
                window(Window::FIVE_HOURS, 39.0),
                window(Window::SEVEN_DAYS, 12.0)
            ]
        );
    }
}

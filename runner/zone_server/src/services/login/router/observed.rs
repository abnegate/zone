use std::collections::HashMap;

use abnegate_llm::Window;
use uuid::Uuid;

use crate::state::AppState;

/// The usage windows the agent reported while a turn ran, the latest of each name on each login
/// the turn ran on, held until they are recorded in one write per login.
#[derive(Debug, Default)]
pub struct Observed {
    windows: HashMap<Uuid, Vec<Window>>,
}

impl Observed {
    /// Holds `window`, as the turn on `login` reported it, over the one of the same name.
    pub fn observe(&mut self, login: Uuid, window: Window) {
        let windows = self.windows.entry(login).or_default();
        match windows
            .iter_mut()
            .find(|current| current.name == window.name)
        {
            Some(current) => *current = window,
            None => windows.push(window),
        }
    }

    /// What the turn observed of `login`, which is then let go of.
    pub fn take(&mut self, login: Uuid) -> Vec<Window> {
        self.windows.remove(&login).unwrap_or_default()
    }

    /// Records what the turn observed of `login` over its snapshot, as [`super::observe`] does,
    /// and lets it go only once that write returns, so a record cancelled part way through
    /// leaves the windows for the next one.
    pub async fn record(&mut self, state: &AppState, login: Uuid) {
        let windows = self.windows.get(&login).map_or(&[][..], Vec::as_slice);
        super::observe(state, login, windows).await;
        self.windows.remove(&login);
    }
}

#[cfg(test)]
mod tests {
    use abnegate_agent_cli::AgentKind;
    use futures::FutureExt;
    use sqlx::PgPool;
    use zone_core::llm::window;

    use super::*;
    use crate::db::agent_logins::{self, Insert};

    fn measured(name: &str, used_percent: f64) -> Window {
        Window::new(name).with_used_percent(used_percent)
    }

    #[test]
    fn a_turn_keeps_the_latest_window_of_each_name_on_each_login() {
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        let mut observed = Observed::default();

        for used in 0..40 {
            observed.observe(first, measured(window::FIVE_HOURS, f64::from(used)));
        }
        observed.observe(first, measured(window::SEVEN_DAYS, 12.0));
        observed.observe(second, measured(window::FIVE_HOURS, 3.0));

        assert_eq!(
            observed.take(first),
            [
                measured(window::FIVE_HOURS, 39.0),
                measured(window::SEVEN_DAYS, 12.0)
            ]
        );
        assert_eq!(observed.take(first), [], "what was taken is let go of");
        assert_eq!(observed.take(second), [measured(window::FIVE_HOURS, 3.0)]);
    }

    #[tokio::test]
    async fn a_record_cancelled_before_it_writes_keeps_the_windows_for_the_next() {
        let pool = PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
        )
        .await
        .expect("the test database");
        let organization = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations (id, name, slug) VALUES ($1, 'Observed', $1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("an organization");
        let login = agent_logins::insert(
            &pool,
            &Insert {
                organization_id: organization,
                agent: AgentKind::Claude.as_str(),
                account: Some("observed"),
                credential: None,
                label: Some("observed"),
                expires_at: None,
            },
        )
        .await
        .expect("a login")
        .id;
        let state = AppState::new(crate::state::test_config(), pool.clone(), None);
        let mut observed = Observed::default();
        observed.observe(login, measured(window::FIVE_HOURS, 40.0));

        let held = super::super::credential::hold(login).await;
        let cancelled = observed.record(&state, login).now_or_never();
        drop(held);
        observed.record(&state, login).await;
        let recorded = agent_logins::get(&pool, login)
            .await
            .expect("the login to be readable")
            .expect("the login")
            .snapshot();
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(organization)
            .execute(&pool)
            .await
            .expect("the organization to be removed");

        assert_eq!(
            cancelled, None,
            "the first record waits on the held login and is dropped there"
        );
        assert_eq!(
            recorded.map(|snapshot| snapshot.windows),
            Some(vec![measured(window::FIVE_HOURS, 40.0)]),
            "the next record writes what the cancelled one left"
        );
        assert_eq!(
            observed.take(login),
            [],
            "a finished record lets go of the windows"
        );
    }
}

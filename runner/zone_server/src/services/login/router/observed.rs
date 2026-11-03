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

    /// Records what the turn observed of `login` over its snapshot, as [`super::observe`] does.
    pub async fn record(&mut self, state: &AppState, login: Uuid) {
        super::observe(state, login, &self.take(login)).await;
    }
}

#[cfg(test)]
mod tests {
    use zone_core::llm::window;

    use super::*;

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
}

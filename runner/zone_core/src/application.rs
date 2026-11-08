//! The name Zone's tools keep their own files under, as `.zone/` inside the
//! working directory: background job logs live in `.zone/jobs`.

use std::sync::LazyLock;

use abnegate_agent::Application;

/// Zone's application name.
pub const APPLICATION: &str = "zone";

static ZONE: LazyLock<Application> =
    LazyLock::new(|| Application::new(APPLICATION).expect("zone is a valid application name"));

/// Zone's [`Application`], for every tool context Zone builds.
pub fn application() -> Application {
    ZONE.clone()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use abnegate_agent::tool::job::log_path;

    use super::*;

    #[test]
    fn the_application_is_zone() {
        assert_eq!(application().as_str(), APPLICATION);
        assert_eq!(APPLICATION, "zone");
    }

    #[test]
    fn job_logs_land_in_dot_zone_jobs() {
        let checkout = Path::new("/srv/checkout");

        assert_eq!(
            log_path(checkout, &application(), "job_x"),
            Path::new("/srv/checkout/.zone/jobs/job_x.log")
        );
    }
}

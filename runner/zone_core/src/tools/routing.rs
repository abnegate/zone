//! Where the HTTP clients a tool or an MCP server starts send their requests.

use std::env;

use abnegate_exec::Proxy;

/// Names the proxy every process a tool starts is routed through. Absent or
/// empty leaves each process's own environment alone.
pub const PROXY_URL_VARIABLE: &str = "TOOL_RUNNER_PROXY_URL";

/// Loopback and the stack's own services, which stay direct behind the proxy.
pub const BYPASS: &[&str] = &[
    "localhost",
    "127.0.0.1",
    "::1",
    "host.docker.internal",
    "gateway.docker.internal",
    "gluetun",
    "searxng",
    "manager",
    "console",
    "litellm",
    "ollama",
    "comfyui",
    "postgres",
    "valkey",
    "traefik",
    "prometheus",
    "grafana",
    ".svc",
    ".svc.cluster.local",
];

/// Route through [`PROXY_URL_VARIABLE`], reaching [`BYPASS`] directly.
///
/// Without it, the executor's own variables decide, which is how a process
/// started under a routed tool routes its own children the same way: the
/// proxy hands them `ABNEGATE_EXEC_PROXY_URL` and `ABNEGATE_EXEC_PROXY_BYPASS`.
pub fn proxy() -> Proxy {
    match env::var_os(PROXY_URL_VARIABLE).filter(|url| !url.is_empty()) {
        Some(url) => Proxy::new(url).with_bypass(BYPASS.iter().copied()),
        None => Proxy::from_environment(),
    }
}

#[cfg(test)]
mod tests {
    use abnegate_exec::{PROXY_BYPASS_VARIABLE, PROXY_URL_VARIABLE as EXECUTOR_PROXY_URL_VARIABLE};
    use tokio::process::Command;

    use super::*;

    const CHILD: &str = "ZONE_ROUTING_TEST_CHILD";
    const PROXY: &str = "http://127.0.0.1:28888";
    const PROXY_VARIABLES: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ];

    /// Run `test` again in a child whose only variables are `PATH` and
    /// `variables`, since the process environment is shared by every test.
    async fn ran_in_child(test: &str, variables: &[(&str, &str)]) -> bool {
        if env::var(CHILD).as_deref() == Ok(test) {
            return false;
        }
        let output = Command::new(env::current_exe().expect("the test binary"))
            .args(["--exact", test, "--nocapture", "--test-threads", "1"])
            .env_clear()
            .env("PATH", env::var_os("PATH").unwrap_or_default())
            .env(CHILD, test)
            .envs(variables.iter().copied())
            .output()
            .await
            .expect("the test binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    async fn routed_environment() -> Vec<String> {
        let mut command = Command::new("env");
        command.env_clear();
        proxy().apply(&mut command);
        let output = command.output().await.expect("env runs");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn the_documented_variable_routes_every_client_and_keeps_the_stack_direct() {
        const NAME: &str = "tools::routing::tests::the_documented_variable_routes_every_client_and_keeps_the_stack_direct";
        if ran_in_child(NAME, &[(PROXY_URL_VARIABLE, PROXY)]).await {
            return;
        }

        let environment = routed_environment().await;

        for name in PROXY_VARIABLES {
            assert!(
                environment.contains(&format!("{name}={PROXY}")),
                "{name}: {environment:?}"
            );
        }
        let bypass = BYPASS.join(",");
        assert!(environment.contains(&format!("NO_PROXY={bypass}")));
        assert!(environment.contains(&format!("no_proxy={bypass}")));
    }

    #[tokio::test]
    async fn a_process_started_under_a_routed_tool_routes_its_own_children_the_same_way() {
        const NAME: &str = "tools::routing::tests::a_process_started_under_a_routed_tool_routes_its_own_children_the_same_way";
        let bypass = BYPASS.join(",");
        if ran_in_child(
            NAME,
            &[
                (EXECUTOR_PROXY_URL_VARIABLE, PROXY),
                (PROXY_BYPASS_VARIABLE, &bypass),
            ],
        )
        .await
        {
            return;
        }

        let environment = routed_environment().await;

        assert!(environment.contains(&format!("HTTPS_PROXY={PROXY}")));
        assert!(environment.contains(&format!("NO_PROXY={bypass}")));
    }

    #[tokio::test]
    async fn without_a_proxy_nothing_is_routed() {
        const NAME: &str = "tools::routing::tests::without_a_proxy_nothing_is_routed";
        if ran_in_child(NAME, &[]).await {
            return;
        }

        let environment = routed_environment().await;

        assert!(
            environment
                .iter()
                .all(|line| !line.to_ascii_lowercase().contains("proxy")),
            "{environment:?}"
        );
    }
}

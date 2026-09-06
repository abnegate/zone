//! Query the Zone Prometheus and Grafana instances from the chat loop.

use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use zone_core::tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::tools::{WorkspaceScope, optional_string_arg, string_arg, truncate};
use crate::config::MonitoringConfig;

const MAX_MONITORING_CHARS: usize = 8_000;
const DASHBOARD_LIMIT: usize = 50;

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    let monitoring = &scope.state.config().monitoring;
    if !monitoring.enabled {
        return;
    }
    if !monitoring.prometheus_url.trim().is_empty() {
        registry.register(Arc::new(QueryPrometheusTool {
            scope: scope.clone(),
            base: monitoring.prometheus_url.clone(),
        }));
    }
    if !monitoring.grafana_url.trim().is_empty() {
        registry.register(Arc::new(ListGrafanaDashboardsTool {
            scope: scope.clone(),
            config: monitoring.clone(),
        }));
    }
}

struct QueryPrometheusTool {
    scope: WorkspaceScope,
    base: String,
}

struct ListGrafanaDashboardsTool {
    scope: WorkspaceScope,
    config: MonitoringConfig,
}

#[async_trait]
impl Tool for QueryPrometheusTool {
    fn name(&self) -> &str {
        "query_prometheus"
    }

    fn description(&self) -> &str {
        "Run a PromQL query against Zone's Prometheus. Prefer a bounded time range \
         (start/end as RFC3339 or unix seconds) over an unbounded instant query for \
         on-call questions. This is the live cluster, not chat history."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "PromQL expression."
                },
                "start": {
                    "type": "string",
                    "description": "Range start (RFC3339 or unix seconds). With end, uses query_range."
                },
                "end": {
                    "type": "string",
                    "description": "Range end (RFC3339 or unix seconds)."
                },
                "step": {
                    "type": "string",
                    "description": "Range step (default 60s)."
                }
            },
            "required": ["query"]
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(30)
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl QueryPrometheusTool {
    async fn run(&self, params: Value) -> ToolResult {
        let _ = &self.scope;
        query_prometheus(&self.base, params).await
    }
}

#[async_trait]
impl Tool for ListGrafanaDashboardsTool {
    fn name(&self) -> &str {
        "list_grafana_dashboards"
    }

    fn description(&self) -> &str {
        "List dashboards on Zone's Grafana. Use a query to search titles. \
         Returns uid, title and URL so you can point the user at the right board."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional Grafana search string."
                }
            }
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(30)
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(self.run(params).await)
    }
}

impl ListGrafanaDashboardsTool {
    async fn run(&self, params: Value) -> ToolResult {
        let _ = &self.scope;
        list_grafana_dashboards(&self.config, params).await
    }
}

async fn query_prometheus(base: &str, params: Value) -> ToolResult {
    let query = match string_arg(&params, "query") {
        Ok(query) => query,
        Err(error) => return error,
    };
    let start = optional_string_arg(&params, "start");
    let end = optional_string_arg(&params, "end");
    let step = optional_string_arg(&params, "step").unwrap_or("60s");
    let mut url = match join_api(
        base,
        if start.is_some() && end.is_some() {
            "/api/v1/query_range"
        } else {
            "/api/v1/query"
        },
    ) {
        Ok(url) => url,
        Err(error) => return ToolResult::error(error),
    };
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("query", query);
        if let (Some(start), Some(end)) = (start, end) {
            pairs.append_pair("start", start);
            pairs.append_pair("end", end);
            pairs.append_pair("step", step);
        }
    }
    match get_json(url, None, None, None).await {
        Ok(value) => render_prometheus(value),
        Err(error) => ToolResult::error(error),
    }
}

async fn list_grafana_dashboards(config: &MonitoringConfig, params: Value) -> ToolResult {
    let mut url = match join_api(&config.grafana_url, "/api/search") {
        Ok(url) => url,
        Err(error) => return ToolResult::error(error),
    };
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("type", "dash-db");
        if let Some(query) = optional_string_arg(&params, "query") {
            pairs.append_pair("query", query);
        }
    }
    match get_json(
        url,
        config.grafana_token.as_deref(),
        config.grafana_user.as_deref(),
        config.grafana_password.as_deref(),
    )
    .await
    {
        Ok(Value::Array(rows)) => render_dashboards(rows),
        Ok(_) => ToolResult::error("Grafana returned an unexpected search payload."),
        Err(error) => ToolResult::error(error),
    }
}

fn render_prometheus(value: Value) -> ToolResult {
    ToolResult::success(truncate(&value.to_string(), MAX_MONITORING_CHARS))
}

fn render_dashboards(rows: Vec<Value>) -> ToolResult {
    let total = rows.len();
    let listed: Vec<Value> = rows
        .into_iter()
        .take(DASHBOARD_LIMIT)
        .map(|row| {
            json!({
                "uid": row.get("uid"),
                "title": row.get("title"),
                "url": row.get("url"),
                "folderTitle": row.get("folderTitle"),
            })
        })
        .collect();
    if listed.is_empty() {
        ToolResult::success("No Grafana dashboards matched.")
    } else {
        ToolResult::success(
            json!({
                "dashboards": listed,
                "total": total,
                "complete": total <= DASHBOARD_LIMIT,
            })
            .to_string(),
        )
    }
}

fn join_api(base: &str, path: &str) -> Result<Url, String> {
    let base = Url::parse(base.trim_end_matches('/'))
        .map_err(|_| "The monitoring service URL is invalid.".to_string())?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err("The monitoring service URL must be http or https.".to_string());
    }
    base.join(path.trim_start_matches('/'))
        .or_else(|_| Url::parse(&format!("{}{path}", base.as_str().trim_end_matches('/'))))
        .map_err(|_| "The monitoring request path is invalid.".to_string())
}

async fn get_json(
    url: Url,
    token: Option<&str>,
    user: Option<&str>,
    password: Option<&str>,
) -> Result<Value, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Zone-monitoring-tools")
        .build()
        .map_err(|_| "Could not initialize the monitoring client.".to_string())?;
    let mut request = client.get(url);
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    } else if let Some(user) = user.filter(|user| !user.is_empty()) {
        request = request.basic_auth(user, password);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Monitoring request failed or timed out.".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Monitoring returned HTTP {}.",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "Monitoring returned an invalid response.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn join_api_stays_on_the_configured_origin() {
        let url = join_api("http://prometheus:9090", "/api/v1/query").unwrap();
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("prometheus"));
        assert_eq!(url.path(), "/api/v1/query");
    }

    #[test]
    fn join_api_accepts_a_trailing_slash() {
        let url = join_api("http://prometheus:9090/", "/api/v1/query").unwrap();
        assert_eq!(url.host_str(), Some("prometheus"));
        assert_eq!(url.path(), "/api/v1/query");
    }

    #[test]
    fn join_api_rejects_non_http_schemes() {
        for base in [
            "ftp://prometheus:9090",
            "file:///etc/prometheus",
            "javascript:alert(1)",
        ] {
            let error = join_api(base, "/api/v1/query").unwrap_err();
            assert!(
                error.contains("http or https") || error.contains("invalid"),
                "{base}: {error}"
            );
        }
    }

    #[test]
    fn join_api_rejects_garbage_urls() {
        assert!(join_api("not a url", "/api/v1/query").is_err());
        assert!(join_api("", "/api/v1/query").is_err());
    }

    #[test]
    fn prometheus_payload_is_character_capped() {
        let huge = json!({"status": "success", "data": "x".repeat(20_000)});
        let result = render_prometheus(huge);
        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.ends_with('…'));
        assert!(output.chars().count() <= MAX_MONITORING_CHARS + 1);
        let small = json!({"status": "success", "data": {"result": []}});
        assert_eq!(
            render_prometheus(small.clone()).output.unwrap(),
            small.to_string()
        );
    }

    #[test]
    fn grafana_dashboard_list_caps_unbounded_results() {
        let rows: Vec<Value> = (0..80)
            .map(|index| {
                json!({
                    "uid": format!("d{index}"),
                    "title": format!("Dash {index}"),
                    "url": "/d",
                    "folderTitle": "ops"
                })
            })
            .collect();
        let result = render_dashboards(rows);
        assert!(result.success);
        let value: Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(
            value["dashboards"].as_array().unwrap().len(),
            DASHBOARD_LIMIT
        );
        assert_eq!(value["total"], 80);
        assert_eq!(value["complete"], false);
        assert_eq!(value["dashboards"][0]["uid"], "d0");
        assert_eq!(value["dashboards"][0]["title"], "Dash 0");
    }

    #[test]
    fn grafana_dashboard_list_keeps_small_results_and_empty_message() {
        let result = render_dashboards(vec![json!({
            "uid": "a",
            "title": "Alpha",
            "url": "/d/a",
            "folderTitle": "ops"
        })]);
        let value: Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(value["dashboards"].as_array().unwrap().len(), 1);
        assert_eq!(value["total"], 1);
        assert_eq!(value["complete"], true);
        let empty = render_dashboards(Vec::new());
        assert!(empty.success);
        assert_eq!(
            empty.output.as_deref(),
            Some("No Grafana dashboards matched.")
        );
    }

    #[tokio::test]
    async fn query_prometheus_truncates_live_payloads() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "success",
                "data": {"resultType": "vector", "result": ["x".repeat(20_000)]}
            })))
            .mount(&server)
            .await;
        let result = query_prometheus(&server.uri(), json!({"query": "up"})).await;
        assert!(result.success);
        let output = result.output.unwrap();
        assert!(output.ends_with('…'));
        assert!(output.chars().count() <= MAX_MONITORING_CHARS + 1);
    }

    #[tokio::test]
    async fn list_grafana_dashboards_caps_live_search() {
        let server = MockServer::start().await;
        let rows: Vec<Value> = (0..60)
            .map(|index| json!({"uid": format!("d{index}"), "title": format!("Dash {index}")}))
            .collect();
        Mock::given(method("GET"))
            .and(path("/api/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(rows))
            .mount(&server)
            .await;
        let config = MonitoringConfig {
            grafana_url: server.uri(),
            ..MonitoringConfig::default()
        };
        let result = list_grafana_dashboards(&config, json!({})).await;
        assert!(result.success);
        let value: Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(
            value["dashboards"].as_array().unwrap().len(),
            DASHBOARD_LIMIT
        );
        assert_eq!(value["total"], 60);
        assert_eq!(value["complete"], false);
    }
}

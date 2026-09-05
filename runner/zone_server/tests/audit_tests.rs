//! Integration tests for audit logging functionality

mod common;

use chrono::{Duration, Utc};
use common::{create_test_pool, setup_test_data};
use uuid::Uuid;

#[tokio::test]
async fn test_log_action_basic() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, workspace_id, user_id) = setup_test_data(&pool).await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: Some(workspace_id),
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: Some("192.168.1.1".to_string()),
        user_agent: Some("Mozilla/5.0".to_string()),
    };

    let resource_id = Uuid::new_v4();
    let old_values = serde_json::json!({"status": "active"});
    let new_values = serde_json::json!({"status": "inactive"});

    let log_id = zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "user.updated",
        "user",
        Some(resource_id),
        Some(old_values.clone()),
        Some(new_values.clone()),
    )
    .await
    .expect("Failed to log action");

    assert_ne!(log_id, Uuid::nil());

    // Verify the log was created
    let log = zone_server::db::audit::get_audit_log(&pool, log_id)
        .await
        .expect("Failed to get audit log")
        .expect("Audit log not found");

    assert_eq!(log.id, log_id);
    assert_eq!(log.organization_id, ctx.org_id);
    assert_eq!(log.workspace_id, ctx.workspace_id);
    assert_eq!(log.actor_id, ctx.actor_id);
    assert_eq!(log.actor_email.as_deref(), Some("test@example.com"));
    assert_eq!(log.action, "user.updated");
    assert_eq!(log.resource_type, "user");
    assert_eq!(log.resource_id, Some(resource_id));
    assert_eq!(log.old_values, Some(old_values));
    assert_eq!(log.new_values, Some(new_values));
    assert_eq!(log.ip_address.as_deref(), Some("192.168.1.1/32"));
    assert_eq!(log.user_agent.as_deref(), Some("Mozilla/5.0"));
}

#[tokio::test]
async fn test_log_action_minimal_context() {
    let pool = create_test_pool().await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: None,
        workspace_id: None,
        actor_id: None,
        actor_email: Some("system@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    let log_id = zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "system.backup",
        "system",
        None,
        None,
        None,
    )
    .await
    .expect("Failed to log action");

    let log = zone_server::db::audit::get_audit_log(&pool, log_id)
        .await
        .expect("Failed to get audit log")
        .expect("Audit log not found");

    assert_eq!(log.action, "system.backup");
    assert_eq!(log.resource_type, "system");
    assert_eq!(log.organization_id, None);
    assert_eq!(log.actor_id, None);
}

#[tokio::test]
async fn test_list_audit_logs_basic() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create multiple logs
    for i in 0..5 {
        zone_server::db::audit::log_action(
            &pool,
            &ctx,
            &format!("action.{}", i),
            "test",
            None,
            None,
            None,
        )
        .await
        .expect("Failed to log action");
    }

    let filters = zone_server::db::audit::AuditFilters::default();
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");

    assert_eq!(logs.len(), 5);

    // Should be ordered by created_at DESC
    for i in 0..logs.len() - 1 {
        assert!(logs[i].created_at >= logs[i + 1].created_at);
    }
}

#[tokio::test]
async fn test_list_audit_logs_with_filters() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;
    let resource_id = Uuid::new_v4();

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create logs with different actions and resource types
    zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "user.login",
        "user",
        Some(resource_id),
        None,
        None,
    )
    .await
    .expect("Failed to log action");

    zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "user.logout",
        "user",
        Some(resource_id),
        None,
        None,
    )
    .await
    .expect("Failed to log action");

    zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "workspace.created",
        "workspace",
        None,
        None,
        None,
    )
    .await
    .expect("Failed to log action");

    // Filter by action
    let filters = zone_server::db::audit::AuditFilters {
        action: Some("user.login".to_string()),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].action, "user.login");

    // Filter by resource_type
    let filters = zone_server::db::audit::AuditFilters {
        resource_type: Some("user".to_string()),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 2);

    // Filter by actor_id
    let filters = zone_server::db::audit::AuditFilters {
        actor_id: Some(user_id),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 3);

    // Filter by resource_id
    let filters = zone_server::db::audit::AuditFilters {
        resource_id: Some(resource_id),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 2);
}

#[tokio::test]
async fn test_list_audit_logs_date_range() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create a log
    zone_server::db::audit::log_action(&pool, &ctx, "test.action", "test", None, None, None)
        .await
        .expect("Failed to log action");

    let now = Utc::now();

    // Filter with start_date in the past - should find the log
    let filters = zone_server::db::audit::AuditFilters {
        start_date: Some(now - Duration::hours(1)),
        end_date: Some(now + Duration::hours(1)),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 1);

    // Filter with start_date in the future - should not find the log
    let filters = zone_server::db::audit::AuditFilters {
        start_date: Some(now + Duration::hours(1)),
        ..Default::default()
    };
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 0);
}

#[tokio::test]
async fn test_list_audit_logs_pagination() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create 15 logs
    for i in 0..15 {
        zone_server::db::audit::log_action(
            &pool,
            &ctx,
            &format!("action.{}", i),
            "test",
            None,
            None,
            None,
        )
        .await
        .expect("Failed to log action");
    }

    let filters = zone_server::db::audit::AuditFilters::default();

    // Get first page
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 10);

    // Get second page
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 10)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 5);

    // Get beyond available logs
    let logs = zone_server::db::audit::list_audit_logs(&pool, org_id, &filters, 10, 20)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs.len(), 0);
}

#[tokio::test]
async fn test_count_audit_logs() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create 5 logs
    for i in 0..5 {
        zone_server::db::audit::log_action(
            &pool,
            &ctx,
            &format!("action.{}", i),
            "test",
            None,
            None,
            None,
        )
        .await
        .expect("Failed to log action");
    }

    let filters = zone_server::db::audit::AuditFilters::default();
    let count = zone_server::db::audit::count_audit_logs(&pool, org_id, &filters)
        .await
        .expect("Failed to count audit logs");
    assert_eq!(count, 5);

    // Count with filters
    let filters = zone_server::db::audit::AuditFilters {
        action: Some("action.1".to_string()),
        ..Default::default()
    };
    let count = zone_server::db::audit::count_audit_logs(&pool, org_id, &filters)
        .await
        .expect("Failed to count audit logs");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_export_audit_logs_csv() {
    let pool = create_test_pool().await;

    // Create real org, workspace, and user
    let (org_id, _workspace_id, user_id) = setup_test_data(&pool).await;
    let resource_id = Uuid::new_v4();

    let ctx = zone_server::db::audit::AuditContext {
        org_id: Some(org_id),
        workspace_id: None,
        actor_id: Some(user_id),
        actor_email: Some("test@example.com".to_string()),
        ip_address: Some("192.168.1.1".to_string()),
        user_agent: Some("Mozilla/5.0".to_string()),
    };

    // Create a couple of logs
    zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "user.login",
        "user",
        Some(resource_id),
        None,
        Some(serde_json::json!({"status": "success"})),
    )
    .await
    .expect("Failed to log action");

    zone_server::db::audit::log_action(
        &pool,
        &ctx,
        "user.logout",
        "user",
        Some(resource_id),
        None,
        None,
    )
    .await
    .expect("Failed to log action");

    let now = Utc::now();
    let start_date = now - Duration::hours(1);
    let end_date = now + Duration::hours(1);

    let csv = zone_server::db::audit::export_audit_logs_csv(&pool, org_id, start_date, end_date)
        .await
        .expect("Failed to export audit logs");

    // Verify CSV structure
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines.len() >= 3); // Header + 2 data rows

    // Verify header
    let header = lines[0];
    assert!(header.contains("id"));
    assert!(header.contains("action"));
    assert!(header.contains("resource_type"));
    assert!(header.contains("actor_email"));
    assert!(header.contains("created_at"));

    // Verify data rows contain expected values
    let data = lines[1..].join("\n");
    assert!(data.contains("user.login"));
    assert!(data.contains("user.logout"));
    assert!(data.contains("test@example.com"));
}

#[tokio::test]
async fn test_get_audit_log_not_found() {
    let pool = create_test_pool().await;

    let result = zone_server::db::audit::get_audit_log(&pool, Uuid::new_v4())
        .await
        .expect("Failed to query audit log");

    assert!(result.is_none());
}

#[tokio::test]
async fn test_audit_logs_isolation_by_org() {
    let pool = create_test_pool().await;

    // Create real organizations and users
    let (org1, _ws1, user1) = setup_test_data(&pool).await;
    let (org2, _ws2, user2) = setup_test_data(&pool).await;

    let ctx1 = zone_server::db::audit::AuditContext {
        org_id: Some(org1),
        workspace_id: None,
        actor_id: Some(user1),
        actor_email: Some("org1@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    let ctx2 = zone_server::db::audit::AuditContext {
        org_id: Some(org2),
        workspace_id: None,
        actor_id: Some(user2),
        actor_email: Some("org2@example.com".to_string()),
        ip_address: None,
        user_agent: None,
    };

    // Create logs for both orgs
    zone_server::db::audit::log_action(&pool, &ctx1, "org1.action", "test", None, None, None)
        .await
        .expect("Failed to log action");

    zone_server::db::audit::log_action(&pool, &ctx2, "org2.action", "test", None, None, None)
        .await
        .expect("Failed to log action");

    // Each org should only see their own logs
    let filters = zone_server::db::audit::AuditFilters::default();

    let logs1 = zone_server::db::audit::list_audit_logs(&pool, org1, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs1.len(), 1);
    assert_eq!(logs1[0].action, "org1.action");

    let logs2 = zone_server::db::audit::list_audit_logs(&pool, org2, &filters, 10, 0)
        .await
        .expect("Failed to list audit logs");
    assert_eq!(logs2.len(), 1);
    assert_eq!(logs2[0].action, "org2.action");
}

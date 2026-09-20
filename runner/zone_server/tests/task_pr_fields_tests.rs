//! Task rows carry the pull request fields the PR worker writes.

mod common;

mod db_tests {
    use uuid::Uuid;

    #[test]
    fn test_task_pr_fields_in_row() {
        // This test verifies the TaskRow struct has the PR fields
        // The actual DB tests would require integration testing
        use zone_server::db::tasks::TaskRow;

        // Create a mock TaskRow to verify the fields exist
        let task = TaskRow {
            id: Uuid::new_v4(),
            created_by: None,
            workspace_id: Uuid::new_v4(),
            project_ids: vec![Uuid::new_v4()],
            title: "Test".to_string(),
            description: "Test desc".to_string(),
            acceptance_criteria: None,
            status: "created".to_string(),
            priority: None,
            model_name: None,
            dependencies: None,
            is_agentic: false,
            require_plan_approval: false,
            github_repo_url: None,
            source_id: None,
            source_ids: None,
            worker_id: None,
            queued_at: None,
            started_at: None,
            completed_at: None,
            created_at: None,
            updated_at: None,
            // PR fields
            pr_url: Some("https://github.com/owner/repo/pull/1".to_string()),
            branch_name: Some("zone/task-123-test".to_string()),
            pr_status: Some("open".to_string()),
            pr_created_at: None,
        };

        assert_eq!(
            task.pr_url,
            Some("https://github.com/owner/repo/pull/1".to_string())
        );
        assert_eq!(task.branch_name, Some("zone/task-123-test".to_string()));
        assert_eq!(task.pr_status, Some("open".to_string()));
    }
}

/// The reception sweep reads a pull request back after it merges; what it
/// learns has to reach the task the console shows, not only the run's
/// artifacts.
#[tokio::test]
async fn a_reception_moves_the_tasks_pr_status_and_only_when_it_changes() {
    use zone_server::db::tasks;

    let pool = common::create_test_pool().await;
    let (_organization, workspace, _user) = common::setup_test_data(&pool).await;
    let task = tasks::create_task(
        &pool,
        workspace,
        &[],
        "Ships a change",
        "Opens a pull request",
        None,
        None,
        true,
        None,
    )
    .await
    .unwrap();

    assert!(
        !tasks::update_task_pr_status(&pool, task.id, "merged")
            .await
            .unwrap(),
        "a task without a pull request has no status to move"
    );

    sqlx::query("UPDATE tasks SET pr_url = 'https://github.com/acme/project/pull/7', pr_status = 'open' WHERE id = $1")
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();

    assert!(
        tasks::update_task_pr_status(&pool, task.id, "merged")
            .await
            .unwrap()
    );
    let read = tasks::get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(read.pr_status.as_deref(), Some("merged"));

    assert!(
        !tasks::update_task_pr_status(&pool, task.id, "merged")
            .await
            .unwrap(),
        "a status that already reads merged is not rewritten"
    );

    sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(task.id)
        .execute(&pool)
        .await
        .unwrap();
}

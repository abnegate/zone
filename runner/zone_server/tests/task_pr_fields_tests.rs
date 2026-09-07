//! Task rows carry the pull request fields the PR worker writes.

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

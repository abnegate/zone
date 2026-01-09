//! PR creation tests
//!
//! Tests for the PR creation workflow including git operations and GitHub API.

use zone_server::services::git::GitService;
use zone_server::services::pr::PrService;

mod git_service_tests {
    use super::*;

    #[test]
    fn test_generate_branch_name_basic() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "Fix the login bug");

        assert!(branch.starts_with("zone/task-12345678-"));
        assert!(branch.contains("fix"));
        assert!(branch.contains("login"));
        assert!(branch.contains("bug"));
        // Should not have uppercase
        assert_eq!(branch, branch.to_lowercase());
    }

    #[test]
    fn test_generate_branch_name_special_characters() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "Add user@email validation!!!");

        // Special characters should be replaced with hyphens
        assert!(!branch.contains('@'));
        assert!(!branch.contains('!'));
        // Multiple hyphens should be collapsed
        assert!(!branch.contains("--"));
    }

    #[test]
    fn test_generate_branch_name_long_title() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let long_title = "A".repeat(200);
        let branch = service.generate_branch_name(task_id, &long_title);

        // Branch name should be truncated to max 100 chars
        assert!(branch.len() <= 100);
    }

    #[test]
    fn test_generate_branch_name_unicode() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "修复登录问题");

        // Non-ASCII chars should be replaced
        assert!(branch.is_ascii());
        assert!(branch.starts_with("zone/task-12345678-"));
    }

    #[test]
    fn test_generate_branch_name_empty_title() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "");

        // Should still have a valid branch name with just the task ID prefix
        assert!(branch.starts_with("zone/task-12345678-"));
    }

    #[test]
    fn test_generate_branch_name_spaces_only() {
        let service = GitService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "   ");

        // Should still produce a valid branch name
        assert!(branch.starts_with("zone/task-12345678-"));
        assert!(!branch.contains(' '));
    }
}

mod pr_service_tests {
    use super::*;

    #[test]
    fn test_parse_github_https_url() {
        let service = PrService::new();

        let (owner, repo) = service
            .parse_github_url("https://github.com/acme-corp/my-project")
            .unwrap();

        assert_eq!(owner, "acme-corp");
        assert_eq!(repo, "my-project");
    }

    #[test]
    fn test_parse_github_https_url_with_git_suffix() {
        let service = PrService::new();

        let (owner, repo) = service
            .parse_github_url("https://github.com/owner/repo.git")
            .unwrap();

        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_github_ssh_url() {
        let service = PrService::new();

        let (owner, repo) = service
            .parse_github_url("git@github.com:owner/repo.git")
            .unwrap();

        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_github_ssh_url_without_git_suffix() {
        let service = PrService::new();

        let (owner, repo) = service
            .parse_github_url("git@github.com:owner/repo")
            .unwrap();

        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_github_invalid_url() {
        let service = PrService::new();

        let result = service.parse_github_url("not-a-github-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_github_gitlab_url() {
        let service = PrService::new();

        // GitLab URLs should fail (we only support GitHub for now)
        let result = service.parse_github_url("https://gitlab.com/owner/repo");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_pr_title() {
        let service = PrService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let title = service.generate_pr_title("Fix authentication bug", task_id);

        assert!(title.contains("[Zone]"));
        assert!(title.contains("Fix authentication bug"));
        assert!(title.contains("12345678"));
    }

    #[test]
    fn test_generate_pr_body_with_all_fields() {
        let service = PrService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body = service.generate_pr_body(
            "Fix authentication bug",
            "The login form was not validating emails correctly",
            task_id,
            Some("- Modified `auth.rs`\n- Updated `login.html`"),
            Some("https://zone.example.com/tasks/12345678"),
        );

        assert!(body.contains("## Summary"));
        assert!(body.contains("Fix authentication bug"));
        assert!(body.contains("not validating emails"));
        assert!(body.contains("## Changes"));
        assert!(body.contains("auth.rs"));
        assert!(body.contains("View in Zone"));
        assert!(body.contains("zone.example.com"));
    }

    #[test]
    fn test_generate_pr_body_without_zone_url() {
        let service = PrService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body = service.generate_pr_body("Task title", "Task description", task_id, None, None);

        // Should include task ID instead of link
        assert!(body.contains("Zone Task ID"));
        assert!(body.contains("12345678"));
        assert!(!body.contains("View in Zone"));
    }

    #[test]
    fn test_generate_pr_body_without_changes() {
        let service = PrService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body = service.generate_pr_body(
            "Task title",
            "Task description",
            task_id,
            None,
            Some("https://zone.example.com/tasks/123"),
        );

        // Should not have Changes section if no diff summary
        assert!(!body.contains("## Changes"));
    }

    #[test]
    fn test_generate_pr_body_contains_footer() {
        let service = PrService::new();
        let task_id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body = service.generate_pr_body("Task", "Description", task_id, None, None);

        assert!(body.contains("automatically created by Zone"));
    }
}

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
            project_id: Uuid::new_v4(),
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
            workspace_id: None,
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

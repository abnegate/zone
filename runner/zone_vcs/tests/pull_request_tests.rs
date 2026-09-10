//! Git operations and GitHub pull request creation.

use zone_vcs::git::GitService;
use zone_vcs::pull_request::{Description, PrService};
use zone_vcs::subject::{Kind, Subject};

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

    fn task() -> uuid::Uuid {
        uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap()
    }

    /// The title is the change's own subject in the format this history uses,
    /// so a reviewer reads it in a list of pull requests the same way they read
    /// a list of commits.
    #[test]
    fn a_pull_request_is_titled_with_a_conventional_commit_subject() {
        let subject = Subject::new(Kind::Fix, "Validate the email before submit.");

        assert_eq!(
            subject.to_string(),
            "(fix): validate the email before submit"
        );
    }

    #[test]
    fn a_description_carries_the_problem_the_report_and_the_files() {
        let body = Description {
            problem: "The login form was not validating emails correctly",
            report: Some("Validated the address before submit, and covered it with a test."),
            changes: Some("- Modified `auth.rs`\n- Updated `login.html`"),
            task: task(),
            url: Some("https://zone.example.com/tasks/12345678"),
        }
        .render();

        assert!(body.contains("## Problem"), "{body}");
        assert!(body.contains("not validating emails"), "{body}");
        assert!(body.contains("## What changed"), "{body}");
        assert!(body.contains("covered it with a test"), "{body}");
        assert!(body.contains("## Files"), "{body}");
        assert!(body.contains("auth.rs"), "{body}");
        assert!(body.contains("zone.example.com"), "{body}");
    }

    #[test]
    fn a_description_without_a_console_link_names_the_task() {
        let body = Description {
            problem: "Task description",
            report: None,
            changes: None,
            task: task(),
            url: None,
        }
        .render();

        assert!(body.contains("12345678"), "{body}");
        assert!(!body.contains("this task]("), "{body}");
    }

    #[test]
    fn a_description_without_changes_heads_no_files_section() {
        let body = Description {
            problem: "Task description",
            report: Some("Nothing needed changing."),
            changes: None,
            task: task(),
            url: Some("https://zone.example.com/tasks/123"),
        }
        .render();

        assert!(!body.contains("## Files"), "{body}");
    }
}

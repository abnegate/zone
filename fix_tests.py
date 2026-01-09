#!/usr/bin/env python3
"""
Script to fix API integration tests by updating routes to use workspace-scoped endpoints.
"""

import re
import sys

def fix_test_file(filepath):
    with open(filepath, 'r') as f:
        content = f.read()

    original_content = content

    # Track all test functions that need workspace setup
    tests_needing_workspace = []

    # Find all test functions that use /api/projects, /api/tasks, or /api/chats
    # (but not those without auth, as they should fail before route parsing)
    pattern = r'#\[tokio::test\]\s*\nasync fn (test_\w+)\(\) \{[\s\S]*?\n\}'

    def process_test(match):
        test_name = match.group(1)
        test_body = match.group(0)

        # Skip tests that are explicitly testing without auth (they should get 401)
        if 'without_auth' in test_name:
            return test_body

        # Check if this test uses old project/task/chat routes
        needs_workspace = (
            '"/api/projects' in test_body or
            '"/api/tasks' in test_body or
            '"/api/chats' in test_body
        ) and 'workspaces/{' not in test_body

        if not needs_workspace:
            return test_body

        # Check if already has setup_test_workspace
        has_setup = 'setup_test_workspace' in test_body

        if not has_setup:
            # Add workspace setup after token
            # Find where we get the token
            token_pattern = r'(let token = get_auth_token\(&client\)\.await;)'
            if re.search(token_pattern, test_body):
                test_body = re.sub(
                    token_pattern,
                    r'\1\n    let (_org_id, workspace_id) = setup_test_workspace(&client, &token).await;',
                    test_body
                )

        # Replace routes
        # /api/projects -> /api/workspaces/{workspace_id}/projects
        test_body = re.sub(r'"/api/projects"', '&format!("/api/workspaces/{}/projects", workspace_id)', test_body)
        test_body = re.sub(r'"/api/projects\?', '&format!("/api/workspaces/{}/projects?", workspace_id)', test_body)
        test_body = re.sub(r'"/api/projects/', '&format!("/api/workspaces/{}/projects/', test_body)

        # /api/tasks -> /api/workspaces/{workspace_id}/tasks
        test_body = re.sub(r'"/api/tasks"', '&format!("/api/workspaces/{}/tasks", workspace_id)', test_body)
        test_body = re.sub(r'"/api/tasks\?', '&format!("/api/workspaces/{}/tasks?", workspace_id)', test_body)
        test_body = re.sub(r'"/api/tasks/', '&format!("/api/workspaces/{}/tasks/', test_body)

        # /api/chats -> /api/workspaces/{workspace_id}/chats
        test_body = re.sub(r'"/api/chats"', '&format!("/api/workspaces/{}/chats", workspace_id)', test_body)
        test_body = re.sub(r'"/api/chats\?', '&format!("/api/workspaces/{}/chats?", workspace_id)', test_body)
        test_body = re.sub(r'"/api/chats/', '&format!("/api/workspaces/{}/chats/', test_body)

        # Fix double &format! issues (from already having format!)
        test_body = test_body.replace('&format!("/api/workspaces/{}/projects/{}", project_id)',
                                      '&format!("/api/workspaces/{}/projects/{}", workspace_id, project_id)')
        test_body = test_body.replace('&format!("/api/workspaces/{}/tasks/{}", task_id)',
                                      '&format!("/api/workspaces/{}/tasks/{}", workspace_id, task_id)')
        test_body = test_body.replace('&format!("/api/workspaces/{}/chats/{}", chat_id)',
                                      '&format!("/api/workspaces/{}/chats/{}", workspace_id, chat_id)')

        # Fix format strings that already had format
        test_body = test_body.replace('&format!(&format!("/api/workspaces/{}/projects/', '&format!("/api/workspaces/{}/projects/')
        test_body = test_body.replace('&format!(&format!("/api/workspaces/{}/tasks/', '&format!("/api/workspaces/{}/tasks/')
        test_body = test_body.replace('&format!(&format!("/api/workspaces/{}/chats/', '&format!("/api/workspaces/{}/chats/')

        return test_body

    # Process all tests
    content = re.sub(pattern, process_test, content)

    # Write back
    if content != original_content:
        with open(filepath, 'w') as f:
            f.write(content)
        print(f"Updated {filepath}")
        return True
    else:
        print(f"No changes needed for {filepath}")
        return False

if __name__ == '__main__':
    filepath = sys.argv[1] if len(sys.argv) > 1 else '/Users/jakebarnby/Local/zone/runner/zone_server/tests/api_tests.rs'
    fix_test_file(filepath)

import { test, expect } from '@playwright/test';

// Mock data generators
const generateMockProject = (
  id: string,
  name: string,
  status: 'active' | 'on_hold' | 'cancelled' = 'active',
  options: { description?: string; github_repo_url?: string } = {}
) => ({
  id,
  name,
  description: options.description || null,
  status,
  github_repo_url: options.github_repo_url || null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

test.describe('Projects Page', () => {
  test.beforeEach(async ({ page }) => {
    // Default mock for models (needed for navigation)
    await page.route('**/api/models', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ models: [] }),
      });
    });

    // Default mock for projects - empty list
    await page.route('**/api/projects*', (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: [] }),
        });
      } else {
        route.continue();
      }
    });

    // Set API key and navigate
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible({ timeout: 10000 });

    // Navigate to projects page
    await page.click('a[href="/projects"]');
    await expect(page).toHaveURL('/projects');
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(page.locator('.page-header h1')).toContainText('Projects');
      await expect(page.locator('.page-header .subtitle')).toContainText('GitHub integration');
    });

    test('shows new project button', async ({ page }) => {
      await expect(page.locator('.page-header .btn-primary')).toContainText('New Project');
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no projects exist', async ({ page }) => {
      await expect(page.locator('.projects-empty')).toBeVisible();
      await expect(page.locator('.projects-empty h3')).toContainText('No projects yet');
    });

    test('shows create project button in empty state', async ({ page }) => {
      await expect(page.locator('.projects-empty .btn-primary')).toContainText('Create Project');
    });
  });

  test.describe('Project List', () => {
    test('displays grid of project cards', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Frontend App', 'active', { description: 'React frontend' }),
        generateMockProject('proj-2', 'Backend API', 'active', { description: 'Node.js API' }),
        generateMockProject('proj-3', 'Mobile App', 'on_hold'),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.project-card')).toHaveCount(3);
    });

    test('displays project name and description', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'My Project', 'active', { description: 'Project description here' }),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.project-name')).toContainText('My Project');
      await expect(page.locator('.project-description')).toContainText('Project description here');
    });

    test('displays status badge with correct color', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Active Project', 'active'),
        generateMockProject('proj-2', 'On Hold Project', 'on_hold'),
        generateMockProject('proj-3', 'Cancelled Project', 'cancelled'),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.status-active')).toContainText('Active');
      await expect(page.locator('.status-on-hold')).toContainText('On Hold');
      await expect(page.locator('.status-cancelled')).toContainText('Cancelled');
    });

    test('displays GitHub link when present', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'GitHub Project', 'active', {
          github_repo_url: 'https://github.com/user/repo',
        }),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.github-link')).toBeVisible();
      await expect(page.locator('.github-link')).toHaveAttribute('href', 'https://github.com/user/repo');
    });

    test('shows "No GitHub" when not linked', async ({ page }) => {
      const mockProjects = [generateMockProject('proj-1', 'Local Project', 'active')];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.no-github')).toContainText('No GitHub');
    });
  });

  test.describe('Status Filter', () => {
    test('displays filter buttons', async ({ page }) => {
      await expect(page.locator('.filter-btn')).toHaveCount(4);
      await expect(page.locator('.filter-btn').nth(0)).toContainText('All');
      await expect(page.locator('.filter-btn').nth(1)).toContainText('Active');
      await expect(page.locator('.filter-btn').nth(2)).toContainText('On Hold');
      await expect(page.locator('.filter-btn').nth(3)).toContainText('Cancelled');
    });

    test('filters by active status', async ({ page }) => {
      const activeProjects = [generateMockProject('proj-1', 'Active Project', 'active')];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        const url = new URL(route.request().url());
        const status = url.searchParams.get('status');

        if (status === 'active') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: activeProjects }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [] }),
          });
        }
      });

      await page.click('.filter-btn:has-text("Active")');
      await expect(page.locator('.filter-btn.active')).toContainText('Active');
    });

    test('filters by on_hold status', async ({ page }) => {
      await page.click('.filter-btn:has-text("On Hold")');
      await expect(page.locator('.filter-btn.active')).toContainText('On Hold');
    });

    test('filters by cancelled status', async ({ page }) => {
      await page.click('.filter-btn:has-text("Cancelled")');
      await expect(page.locator('.filter-btn.active')).toContainText('Cancelled');
    });

    test('All filter is selected by default', async ({ page }) => {
      await expect(page.locator('.filter-btn.active')).toContainText('All');
    });
  });

  test.describe('Create Project', () => {
    test('opens create modal from header button', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await expect(page.locator('.modal-content h3')).toContainText('New Project');
    });

    test('opens create modal from empty state button', async ({ page }) => {
      await page.click('.projects-empty .btn-primary');
      await expect(page.locator('.modal-content h3')).toContainText('New Project');
    });

    test('shows all form fields', async ({ page }) => {
      await page.click('.page-header .btn-primary');

      await expect(page.locator('#project-name')).toBeVisible();
      await expect(page.locator('#project-description')).toBeVisible();
      await expect(page.locator('#project-status')).toBeVisible();
      await expect(page.locator('#project-github')).toBeVisible();
    });

    test('creates project with minimum fields', async ({ page }) => {
      const newProject = generateMockProject('new-proj', 'My New Project', 'active');

      await page.route('**/api/projects', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: newProject }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#project-name', 'My New Project');
      await page.click('.modal-actions .btn-primary');

      // Modal should close on success
      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('creates project with all fields', async ({ page }) => {
      const newProject = generateMockProject('new-proj', 'Full Project', 'on_hold', {
        description: 'Full description',
        github_repo_url: 'https://github.com/test/repo',
      });

      await page.route('**/api/projects', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: newProject }),
          });
        }
      });

      await page.click('.page-header .btn-primary');
      await page.fill('#project-name', 'Full Project');
      await page.fill('#project-description', 'Full description');
      await page.selectOption('#project-status', 'on_hold');
      await page.fill('#project-github', 'https://github.com/test/repo');
      await page.click('.modal-actions .btn-primary');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('disables create button when name is empty', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await expect(page.locator('.modal-actions .btn-primary')).toBeDisabled();
    });

    test('enables create button when name is entered', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await page.fill('#project-name', 'Test');
      await expect(page.locator('.modal-actions .btn-primary')).not.toBeDisabled();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.click('.page-header .btn-primary');
      await page.click('.modal-actions .btn-secondary');
      await expect(page.locator('.modal-content')).not.toBeVisible();
    });
  });

  test.describe('Project Details Sidebar', () => {
    const mockProject = generateMockProject('proj-1', 'Test Project', 'active', {
      description: 'Test description',
    });

    test.beforeEach(async ({ page }) => {
      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');
    });

    test('opens details sidebar on card click', async ({ page }) => {
      await page.click('.project-card');
      await expect(page.locator('.project-details')).toBeVisible();
    });

    test('displays project details', async ({ page }) => {
      await page.click('.project-card');

      await expect(page.locator('.project-details h2')).toContainText('Test Project');
      await expect(page.locator('.project-details')).toContainText('Test description');
      await expect(page.locator('.project-details .status-badge')).toContainText('Active');
    });

    test('shows edit and delete buttons', async ({ page }) => {
      await page.click('.project-card');

      await expect(page.locator('.details-actions .btn-secondary')).toContainText('Edit Project');
      await expect(page.locator('.details-actions .btn-danger')).toContainText('Delete');
    });

    test('closes sidebar with close button', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-header .btn-icon');
      await expect(page.locator('.project-details')).not.toBeVisible();
    });

    test('shows link repository button when no GitHub', async ({ page }) => {
      await page.click('.project-card');
      await expect(page.locator('.detail-row:has-text("GitHub") .btn-secondary')).toContainText('Link Repository');
    });
  });

  test.describe('Edit Project', () => {
    const mockProject = generateMockProject('proj-1', 'Original Name', 'active', {
      description: 'Original description',
    });

    test.beforeEach(async ({ page }) => {
      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        const method = route.request().method();
        const url = route.request().url();

        if (method === 'GET' && !url.includes('/proj-1')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        } else if (method === 'PATCH') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              success: true,
              project: { ...mockProject, name: 'Updated Name' },
            }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');
    });

    test('opens edit modal with pre-filled values', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-secondary');

      await expect(page.locator('.modal-content h3')).toContainText('Edit Project');
      await expect(page.locator('#edit-name')).toHaveValue('Original Name');
      await expect(page.locator('#edit-description')).toHaveValue('Original description');
    });

    test('updates project successfully', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-secondary');
      await page.fill('#edit-name', 'Updated Name');
      await page.click('.modal-actions .btn-primary');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('closes edit modal on cancel', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-secondary');
      await page.click('.modal-actions .btn-secondary');

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });
  });

  test.describe('Delete Project', () => {
    const mockProject = generateMockProject('proj-1', 'Test Project', 'active');

    test.beforeEach(async ({ page }) => {
      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        const method = route.request().method();

        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        } else if (method === 'DELETE') {
          route.fulfill({ status: 204 });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');
    });

    test('shows delete confirmation modal', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-danger');

      await expect(page.locator('.modal-content h3')).toContainText('Delete Project');
      await expect(page.locator('.modal-content')).toContainText('cannot be undone');
      await expect(page.locator('.modal-content strong')).toContainText('Test Project');
    });

    test('deletes project after confirmation', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-danger');
      await page.click('.modal-actions .btn-danger');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('cancels delete', async ({ page }) => {
      await page.click('.project-card');
      await page.click('.details-actions .btn-danger');
      await page.click('.modal-actions .btn-secondary');

      await expect(page.locator('.modal-content')).not.toBeVisible();
      await expect(page.locator('.project-details')).toBeVisible();
    });
  });

  test.describe('GitHub Integration', () => {
    test('shows link GitHub modal', async ({ page }) => {
      const mockProject = generateMockProject('proj-1', 'Test Project', 'active');

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await page.click('.project-card');
      await page.click('.detail-row:has-text("GitHub") .btn-secondary');

      await expect(page.locator('.modal-content h3')).toContainText('Link GitHub Repository');
    });

    test('links GitHub repository', async ({ page }) => {
      const mockProject = generateMockProject('proj-1', 'Test Project', 'active');
      const linkedProject = { ...mockProject, github_repo_url: 'https://github.com/test/repo' };

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        const method = route.request().method();
        const url = route.request().url();

        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        } else if (method === 'PUT' && url.includes('/github')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: linkedProject }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await page.click('.project-card');
      await page.click('.detail-row:has-text("GitHub") .btn-secondary');
      await page.fill('#github-url', 'https://github.com/test/repo');
      await page.click('.modal-actions .btn-primary');

      await expect(page.locator('.modal-content')).not.toBeVisible({ timeout: 5000 });
    });

    test('unlinks GitHub repository', async ({ page }) => {
      const mockProject = generateMockProject('proj-1', 'Test Project', 'active', {
        github_repo_url: 'https://github.com/test/repo',
      });
      const unlinkedProject = { ...mockProject, github_repo_url: null };

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        const method = route.request().method();
        const url = route.request().url();

        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        } else if (method === 'DELETE' && url.includes('/github')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: unlinkedProject }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await page.click('.project-card');
      await page.click('.github-detail .btn-secondary:has-text("Unlink")');

      // The link should be removed from details
    });

    test('shows GitHub URL in details when linked', async ({ page }) => {
      const mockProject = generateMockProject('proj-1', 'Test Project', 'active', {
        github_repo_url: 'https://github.com/test/repo',
      });

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await page.click('.project-card');
      await expect(page.locator('.github-url')).toContainText('https://github.com/test/repo');
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading projects fails', async ({ page }) => {
      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ success: false, error: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.projects-error')).toBeVisible();
    });
  });

  test.describe('Accessibility', () => {
    test('project cards are keyboard navigable', async ({ page }) => {
      const mockProjects = [generateMockProject('proj-1', 'Test Project', 'active')];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.project-card')).toHaveAttribute('tabindex', '0');
    });

    test('GitHub link opens in new tab', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Test Project', 'active', {
          github_repo_url: 'https://github.com/test/repo',
        }),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.locator('.github-link')).toHaveAttribute('target', '_blank');
      await expect(page.locator('.github-link')).toHaveAttribute('rel', 'noopener noreferrer');
    });
  });

  test.describe('Card Selection', () => {
    test('highlights selected project card', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Project 1', 'active'),
        generateMockProject('proj-2', 'Project 2', 'active'),
      ];

      await page.unroute('/api/projects*');
      await page.route('**/api/projects*', (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: mockProjects }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await page.click('.project-card:first-child');
      await expect(page.locator('.project-card.selected')).toBeVisible();
    });
  });
});

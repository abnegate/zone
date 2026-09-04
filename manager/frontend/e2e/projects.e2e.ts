import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

// Mock data generators
const generateMockProject = (
  id: string,
  name: string,
  status: 'active' | 'on_hold' | 'cancelled' = 'active',
  options: { description?: string; github_repo_url?: string; source_id?: string } = {}
) => ({
  id,
  name,
  description: options.description || null,
  status,
  github_repo_url: options.github_repo_url || null,
  source_id: options.source_id || null,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
});

const sourcesRoutePattern = /\/api\/workspaces\/[^/]+\/sources/;
const sourcesListPattern = /\/api\/workspaces\/[^/]+\/sources\/?$/;
const isSourcesListRequest = (requestUrl: string) =>
  sourcesListPattern.test(new URL(requestUrl).pathname);
const projectsListRoutePattern = /\/api\/projects(?:\?.*)?$/;
const projectDetailRoutePattern = /\/api\/projects\/[^/]+$/;
const projectSyncRoutePattern = /\/api\/projects\/[^/]+\/sync(?:\/[^/]+)?$/;

test.describe('Projects Page', () => {
  test.beforeEach(async ({ context, page }) => {
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);

    // Mock sources endpoint (needed by ProjectsPage)
    await routeApi(page, sourcesRoutePattern, (route) => {
      if (isSourcesListRequest(route.request().url())) {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ sources: [] }),
        });
      } else {
        route.continue();
      }
    });

    // Mock organizations with query params
    await routeApi(page, '**/api/organizations?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          organizations: [
            {
              id: '00000000-0000-0000-0000-000000000001',
              name: 'Default Org',
              slug: 'default',
              description: null,
              is_active: true,
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
        }),
      });
    });

    // Mock workspaces with query params
    await routeApi(page, '**/api/organizations/*/workspaces?*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          workspaces: [
            {
              id: '00000000-0000-0000-0000-000000000001',
              organization_id: '00000000-0000-0000-0000-000000000001',
              name: 'Default Workspace',
              slug: 'default',
              description: null,
              is_active: true,
              created_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
            },
          ],
        }),
      });
    });

    await routeApi(page, projectSyncRoutePattern, (route) => {
      const method = route.request().method();
      const requestUrl = new URL(route.request().url());
      const match = requestUrl.pathname.match(/\/api\/projects\/([^/]+)\/sync/);
      const projectId = match ? match[1] : 'proj-1';

      if (method === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, configs: [] }),
        });
        return;
      }

      if (method === 'POST') {
        route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            success: true,
            config: {
              id: 'sync-1',
              project_id: projectId,
              provider: 'github',
              direction: 'bidirectional',
              external_repo_url: 'https://github.com/example/repo',
              is_active: true,
              created_at: new Date().toISOString(),
            },
          }),
        });
        return;
      }

      if (method === 'DELETE') {
        route.fulfill({ status: 204 });
        return;
      }

      route.continue();
    });

    // Default mock for projects - empty list
    await routeApi(page, projectsListRoutePattern, (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ success: true, projects: [] }),
        });
        return;
      }
      route.continue();
    });

    // Set API key and navigate
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to projects page
    await page.click('a[href="/projects"]');
    await expect(page).toHaveURL('/projects');
    await expect(page.locator('.projects-page')).toBeVisible({ timeout: 10000 });
  });

  test.describe('Page Header', () => {
    test('displays page title and subtitle', async ({ page }) => {
      await expect(
        page.getByRole('heading', { name: 'Projects', exact: true })
      ).toBeVisible();
      await expect(
        page.getByText('Organize work with GitHub integration')
      ).toBeVisible();
    });

    test('shows new project button', async ({ page }) => {
      await expect(page.getByRole('button', { name: '+ New Project' })).toBeVisible();
    });
  });

  test.describe('Empty State', () => {
    test('shows empty state when no projects exist', async ({ page }) => {
      await expect(
        page.getByRole('heading', { name: 'No projects yet' })
      ).toBeVisible();
      await expect(
        page.getByText('Create your first project to get started')
      ).toBeVisible();
    });

    test('shows create project button in empty state', async ({ page }) => {
      await expect(page.getByRole('button', { name: 'Create Project' })).toBeVisible();
    });
  });

  test.describe('Project List', () => {
    test('displays grid of project cards', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Frontend App', 'active', { description: 'React frontend' }),
        generateMockProject('proj-2', 'Backend API', 'active', { description: 'Node.js API' }),
        generateMockProject('proj-3', 'Mobile App', 'on_hold'),
      ];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      await expect(page.locator('.projects-page .ui-card')).toHaveCount(3);
    });

    test('displays project name and description', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'My Project', 'active', { description: 'Project description here' }),
      ];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      const firstCard = page.locator('.projects-page .ui-card').first();
      await expect(firstCard.locator('h3')).toContainText('My Project');
      await expect(firstCard.getByText('Project description here')).toBeVisible();
    });

    test('displays status badge with correct color', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Active Project', 'active'),
        generateMockProject('proj-2', 'On Hold Project', 'on_hold'),
        generateMockProject('proj-3', 'Cancelled Project', 'cancelled'),
      ];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      const cards = page.locator('.projects-page .ui-card');
      await expect(cards.nth(0)).toContainText('Active');
      await expect(cards.nth(1)).toContainText('On Hold');
      await expect(cards.nth(2)).toContainText('Cancelled');
    });

    test('shows "No source" when not linked', async ({ page }) => {
      const mockProjects = [generateMockProject('proj-1', 'Local Project', 'active')];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      await expect(page.locator('.projects-page .ui-card').first()).toContainText('No source');
    });
  });

  test.describe('Status Filter', () => {
    test('displays filter buttons', async ({ page }) => {
      await expect(page.getByRole('tab', { name: 'All' })).toBeVisible();
      await expect(page.getByRole('tab', { name: 'Active' })).toBeVisible();
      await expect(page.getByRole('tab', { name: 'On Hold' })).toBeVisible();
      await expect(page.getByRole('tab', { name: 'Cancelled' })).toBeVisible();
    });

    test('filters by active status', async ({ page }) => {
      const activeProjects = [generateMockProject('proj-1', 'Active Project', 'active')];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      const activeTab = page.getByRole('tab', { name: 'Active' });
      await activeTab.click();
      await expect(activeTab).toHaveAttribute('data-state', 'active');
    });

    test('filters by on_hold status', async ({ page }) => {
      const onHoldTab = page.getByRole('tab', { name: 'On Hold' });
      await onHoldTab.click();
      await expect(onHoldTab).toHaveAttribute('data-state', 'active');
    });

    test('filters by cancelled status', async ({ page }) => {
      const cancelledTab = page.getByRole('tab', { name: 'Cancelled' });
      await cancelledTab.click();
      await expect(cancelledTab).toHaveAttribute('data-state', 'active');
    });

    test('All filter is selected by default', async ({ page }) => {
      await expect(page.getByRole('tab', { name: 'All' })).toHaveAttribute(
        'data-state',
        'active'
      );
    });
  });

  test.describe('Create Project', () => {
    test('opens create modal from header button', async ({ page }) => {
      await page.getByRole('button', { name: '+ New Project' }).click();
      await expect(page.getByRole('dialog', { name: 'New Project' })).toBeVisible();
    });

    test('opens create modal from empty state button', async ({ page }) => {
      await page.getByRole('button', { name: 'Create Project' }).click();
      await expect(page.getByRole('dialog', { name: 'New Project' })).toBeVisible();
    });

    test('shows all form fields', async ({ page }) => {
      await page.getByRole('button', { name: '+ New Project' }).click();

      const dialog = page.getByRole('dialog', { name: 'New Project' });
      await expect(dialog.locator('#project-name')).toBeVisible();
      await expect(dialog.locator('#project-description')).toBeVisible();
      await dialog.locator('#project-name').fill( 'Test Project');
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.wizard-empty-state')).toBeVisible();
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.status-selection-option')).toHaveCount(3);
    });

    test('creates project with minimum fields', async ({ page }) => {
      const newProject = generateMockProject('new-proj', 'My New Project', 'active');

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
        const method = route.request().method();
        if (method === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: newProject }),
          });
          return;
        }
        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [] }),
          });
          return;
        }
        route.continue();
      });

      await page.getByRole('button', { name: '+ New Project' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Project' });
      await dialog.locator('#project-name').fill('My New Project');
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.wizard-empty-state')).toBeVisible();
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.status-selection-option')).toHaveCount(3);
      await dialog.getByRole('button', { name: 'Create Project' }).click();

      // Modal should close on success
      await expect(page.getByRole('dialog', { name: 'New Project' })).toHaveCount(0);
    });

    test('creates project with all fields', async ({ page }) => {
      const newProject = generateMockProject('new-proj', 'Full Project', 'on_hold', {
        description: 'Full description',
      });

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
        const method = route.request().method();
        if (method === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, project: newProject }),
          });
          return;
        }
        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [] }),
          });
          return;
        }
        route.continue();
      });

      await page.getByRole('button', { name: '+ New Project' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Project' });
      await dialog.locator('#project-name').fill('Full Project');
      await dialog.locator('#project-description').fill('Full description');
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.wizard-empty-state')).toBeVisible();
      await dialog.getByRole('button', { name: 'Next' }).click();
      await expect(dialog.locator('.status-selection-option')).toHaveCount(3);
      await dialog.getByRole('button', { name: 'On Hold' }).click();
      await dialog.getByRole('button', { name: 'Create Project' }).click();

      await expect(page.getByRole('dialog', { name: 'New Project' })).toHaveCount(0);
    });

    test('disables create button when name is empty', async ({ page }) => {
      await page.getByRole('button', { name: '+ New Project' }).click();
      await expect(
        page.getByRole('dialog', { name: 'New Project' }).getByRole('button', {
          name: 'Next',
        })
      ).toBeDisabled();
    });

    test('enables create button when name is entered', async ({ page }) => {
      await page.getByRole('button', { name: '+ New Project' }).click();
      const dialog = page.getByRole('dialog', { name: 'New Project' });
      await dialog.locator('#project-name').fill( 'Test');
      await expect(dialog.getByRole('button', { name: 'Next' })).not.toBeDisabled();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.getByRole('button', { name: '+ New Project' }).click();
      await page
        .getByRole('dialog', { name: 'New Project' })
        .getByRole('button', { name: 'Cancel' })
        .click();
      await expect(page.getByRole('dialog', { name: 'New Project' })).toHaveCount(0);
    });
  });

  test.describe('Project Details Sidebar', () => {
    const mockProject = generateMockProject('proj-1', 'Test Project', 'active', {
      description: 'Test description',
    });

    test.beforeEach(async ({ page }) => {
      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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
      await page.locator('.projects-page .ui-card').first().click();
      await expect(page.locator('.project-details')).toBeVisible();
    });

    test('displays project details', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();

      await expect(page.locator('.project-details h2')).toContainText('Test Project');
      await expect(page.locator('.project-details')).toContainText('Test description');
      await expect(page.locator('.project-details')).toContainText('Active');
    });

    test('shows edit and delete buttons', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();

      await expect(
        page.locator('.details-actions').getByRole('button', { name: 'Edit Project' })
      ).toBeVisible();
      await expect(
        page.locator('.details-actions').getByRole('button', { name: 'Delete' })
      ).toBeVisible();
    });

    test('closes sidebar with close button', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await page.locator('.project-details').getByRole('button', { name: 'Close' }).click();
      await expect(page.locator('.project-details')).not.toBeVisible();
    });

    test('shows link source button when no source', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await expect(
        page.locator('.project-details').getByRole('button', { name: 'Link Source' })
      ).toBeVisible();
    });
  });

  test.describe('Edit Project', () => {
    const mockProject = generateMockProject('proj-1', 'Original Name', 'active', {
      description: 'Original description',
    });

    test.beforeEach(async ({ page }) => {
      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
          return;
        }
        route.continue();
      });
      await routeApi(page, projectDetailRoutePattern, (route) => {
        if (route.request().method() === 'PATCH') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              success: true,
              project: { ...mockProject, name: 'Updated Name' },
            }),
          });
          return;
        }
        route.continue();
      });

      await page.reload();
      await page.click('a[href="/projects"]');
    });

    test('opens edit modal with pre-filled values', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await page
        .locator('.details-actions')
        .getByRole('button', { name: 'Edit Project' })
        .click();

      await expect(page.locator('.modal-content h3')).toContainText('Edit Project');
      await expect(page.locator('#edit-name')).toHaveValue('Original Name');
      await expect(page.locator('#edit-description')).toHaveValue('Original description');
    });

    test('closes edit modal on cancel', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await page
        .locator('.details-actions')
        .getByRole('button', { name: 'Edit Project' })
        .click();
      await page.locator('.modal-actions').getByRole('button', { name: 'Cancel' }).click();

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });
  });

  test.describe('Delete Project', () => {
    const mockProject = generateMockProject('proj-1', 'Test Project', 'active');

    test.beforeEach(async ({ page }) => {
      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ success: true, projects: [mockProject] }),
          });
          return;
        }
        route.continue();
      });
      await routeApi(page, projectDetailRoutePattern, (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
          return;
        }
        route.continue();
      });

      await page.reload();
      await page.click('a[href="/projects"]');
    });

    test('shows delete confirmation modal', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await page.locator('.details-actions').getByRole('button', { name: 'Delete' }).click();

      await expect(page.locator('.modal-content h3')).toContainText('Delete Project');
      await expect(page.locator('.modal-content')).toContainText('cannot be undone');
      await expect(page.locator('.modal-content strong')).toContainText('Test Project');
    });

    test('cancels delete', async ({ page }) => {
      await page.locator('.projects-page .ui-card').first().click();
      await page.locator('.details-actions').getByRole('button', { name: 'Delete' }).click();
      await page.locator('.modal-actions').getByRole('button', { name: 'Cancel' }).click();

      await expect(page.locator('.modal-content')).not.toBeVisible();
      await expect(page.locator('.project-details')).toBeVisible();
    });
  });

  test.describe('Source Integration', () => {
    test('shows link source modal', async ({ page }) => {
      const mockProject = generateMockProject('proj-1', 'Test Project', 'active');

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      await page.locator('.projects-page .ui-card').first().click();
      await page.locator('.project-details').getByRole('button', { name: 'Link Source' }).click();

      await expect(page.locator('.modal-content h3')).toContainText('Link Source');
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading projects fails', async ({ page }) => {
      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ message: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/projects"]');

      await expect(page.getByText('Server error')).toBeVisible({ timeout: 15000 });
    });
  });

  test.describe('Accessibility', () => {
    test('project cards are keyboard navigable', async ({ page }) => {
      const mockProjects = [generateMockProject('proj-1', 'Test Project', 'active')];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      await expect(
        page.locator('.projects-page .ui-card').first()
      ).toHaveAttribute('tabindex', '0');
    });
  });

  test.describe('Card Selection', () => {
    test('highlights selected project card', async ({ page }) => {
      const mockProjects = [
        generateMockProject('proj-1', 'Project 1', 'active'),
        generateMockProject('proj-2', 'Project 2', 'active'),
      ];

      await page.unroute(projectsListRoutePattern);
      await routeApi(page, projectsListRoutePattern, (route) => {
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

      const firstCard = page.locator('.projects-page .ui-card').first();
      await firstCard.click();
      await expect(firstCard).toHaveClass(/selected/);
    });
  });
});

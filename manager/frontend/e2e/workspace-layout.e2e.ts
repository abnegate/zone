import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import {
  mockChats,
  setupAdminAuth,
  setupCommonRoutes,
} from './layout-fixtures';
import { blockServiceWorker, routeApi, SOURCES_PATH } from './test-utils';

const TIGHTEST_DESKTOP = { width: 1024, height: 768 };

const screens = [
  {
    path: 'chats',
    heading: 'Chats',
    content: '.chat-item',
    title: '.chat-title',
    titleLeading: '18px',
  },
  {
    path: 'projects',
    heading: 'Projects',
    content: '.project-card',
    title: '.project-name',
    titleLeading: '20px',
  },
  {
    path: 'tasks',
    heading: 'Tasks',
    content: '.task-card:not(.board-skeleton-card)',
    title: '.task-card-open',
    titleLeading: '20px',
  },
  {
    path: 'wiki',
    heading: 'Knowledge Base',
    content: '.knowledge-card',
    title: '.knowledge-card-title',
    titleLeading: '20px',
  },
  {
    path: 'models',
    heading: 'Models',
    content: '.model-item',
    title: '.model-name',
    titleLeading: '20px',
  },
];

async function fitsViewport(page: Page): Promise<void> {
  const width = await page.evaluate(() => ({
    content: document.documentElement.scrollWidth,
    viewport: innerWidth,
  }));
  expect(width.content).toBeLessThanOrEqual(width.viewport);
  const main = await page.locator('main.main-content').evaluate((element) => ({
    scroll: element.scrollWidth,
    client: element.clientWidth,
  }));
  expect(main.scroll).toBeLessThanOrEqual(main.client);
}

async function tableFits(page: Page): Promise<void> {
  const table = await page
    .locator('.sources-table-wrapper')
    .evaluate((element) => ({
      scroll: element.scrollWidth,
      client: element.clientWidth,
    }));
  expect(table.scroll).toBeLessThanOrEqual(table.client);
}

async function capture(
  page: Page,
  name: string,
  theme: 'light' | 'dark'
): Promise<void> {
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
  if (process.env.RUN_SCREENSHOTS === 'true')
    await page.screenshot({
      path: `screenshots/layout/${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}

async function ready(
  page: Page,
  path: string,
  theme: 'light' | 'dark'
): Promise<void> {
  await page.goto(path, { waitUntil: 'domcontentloaded' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
  await page.evaluate(() => document.fonts.ready);
}

for (const viewport of [
  { width: 1440, height: 1000 },
  { width: 390, height: 844 },
]) {
  for (const theme of ['light', 'dark'] as const) {
    const profile = `${viewport.width}-${theme}`;
    test.describe(`Workspace layout ${profile}`, () => {
      test.use({ viewport });
      test.beforeEach(async ({ page, context }) => {
        await blockServiceWorker(context);
        await page.emulateMedia({ colorScheme: theme });
        await setupCommonRoutes(page, true);
        await page.goto('/login', { waitUntil: 'domcontentloaded' });
        await setupAdminAuth(page);
      });
      test.afterEach(async ({ page }) => {
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      });

      test('populated screens share title and list typography without horizontal overflow', async ({
        page,
      }) => {
        for (const screen of screens) {
          await ready(page, `/${screen.path}`, theme);
          const heading = page.getByRole('heading', {
            name: screen.heading,
            exact: true,
          });
          await expect(heading).toBeVisible();
          await expect(page.locator(screen.content).first()).toBeVisible();
          await expect(
            page.locator('.skeleton-card, .board-skeleton-card'),
          ).toHaveCount(0);
          await expect(heading).toHaveCSS('font-size', '18px');
          await expect(heading).toHaveCSS('line-height', '22.5px');
          if (viewport.width > 768)
            await expect(page.locator('.page-bar')).toHaveCSS('height', '48px');
          if (screen.title) {
            await expect(page.locator(screen.title).first()).toHaveCSS(
              'font-size',
              '13px'
            );
            await expect(page.locator(screen.title).first()).toHaveCSS(
              'line-height',
              screen.titleLeading!
            );
          }
          await fitsViewport(page);
          await capture(page, `${profile}-${screen.path}`, theme);
        }
      });

      test('the sources section keeps its table inside the viewport', async ({
        page,
      }) => {
        await ready(page, SOURCES_PATH, theme);
        await expect(
          page
            .getByRole('region', { name: 'Sources', exact: true })
            .getByRole('heading', { level: 2, name: 'Sources', exact: true })
        ).toBeVisible();
        await expect(page.locator('.source-card').first()).toBeVisible();
        await expect(page.locator('.skeleton-card')).toHaveCount(0);
        await expect(page.locator('.source-name').first()).toHaveCSS(
          'font-size',
          '13px'
        );
        await expect(page.locator('.source-name').first()).toHaveCSS(
          'line-height',
          '20px'
        );
        await fitsViewport(page);
        if (viewport.width > 768) await tableFits(page);
        await capture(page, `${profile}-sources`, theme);
      });

      if (viewport.width > 768)
        test('the sources table fits the tightest desktop width without scrolling sideways', async ({
          page,
        }) => {
          await page.setViewportSize(TIGHTEST_DESKTOP);
          await ready(page, SOURCES_PATH, theme);
          await expect(page.locator('.source-card').first()).toBeVisible();
          await expect(page.locator('.skeleton-card')).toHaveCount(0);
          await fitsViewport(page);
          await tableFits(page);
          await expect(
            page
              .locator('.source-card')
              .first()
              .getByRole('button', { name: 'Delete', exact: true })
          ).toBeInViewport();
        });

      for (const wizard of [
        {
          name: 'projects',
          path: '/projects',
          button: 'New project',
          title: 'New Project',
        },
        { name: 'tasks', path: '/tasks', button: 'New task', title: 'New Task' },
        {
          name: 'sources',
          path: SOURCES_PATH,
          button: 'Add source',
          title: 'Add Source',
        },
        {
          name: 'wiki',
          path: '/wiki',
          button: 'Add knowledge',
          title: 'Add Knowledge Entry',
        },
      ]) {
        test(`the ${wizard.name} creation wizard keeps its title, steps, content and footer visible`, async ({
          page,
        }) => {
          await ready(page, wizard.path, theme);
          const button = page.getByRole('button', {
            name: wizard.button,
            exact: true,
          });
          await expect(button).toBeEnabled();
          await button.click();
          const dialog = page.getByRole('dialog');
          await expect(dialog).toBeVisible();
          await expect(
            dialog.getByRole('heading', { name: wizard.title, exact: true })
          ).toHaveCSS('font-size', '18px');
          await expect(
            dialog.getByRole('navigation', { name: 'Wizard steps' })
          ).toBeVisible();
          await expect(dialog.locator('.wizard-step-content')).toBeVisible();
          await expect(
            dialog.getByRole('button', { name: 'Cancel', exact: true })
          ).toBeVisible();
          await expect(
            dialog.getByRole('button', { name: 'Next', exact: true })
          ).toBeVisible();
          const bounds = await dialog.boundingBox();
          expect(bounds).not.toBeNull();
          expect(bounds!.width).toBeLessThanOrEqual(viewport.width - 32);
          expect(bounds!.height).toBeLessThanOrEqual(viewport.height - 32);
          await fitsViewport(page);
          await capture(page, `${profile}-${wizard.name}-wizard`, theme);
          if (wizard.name === 'projects') {
            await dialog.getByLabel('Project Name').fill('Layout review');
          } else if (wizard.name === 'tasks') {
            await dialog.locator('.project-selection-option').first().click();
          } else if (wizard.name === 'sources') {
            await dialog
              .locator('.source-type-option')
              .filter({ hasText: 'Raw text' })
              .click();
          }
          await dialog
            .getByRole('button', { name: 'Next', exact: true })
            .click();
          await expect(dialog.locator('[aria-current="step"]')).toContainText(
            '2'
          );
          await capture(
            page,
            `${profile}-${wizard.name}-wizard-content`,
            theme
          );
          if (wizard.name === 'tasks') {
            await dialog
              .getByLabel('Description', { exact: true })
              .fill('Keep typography and spacing consistent.');
          } else if (wizard.name === 'sources' || wizard.name === 'wiki') {
            await dialog
              .locator('textarea')
              .first()
              .fill('Shared workspace documentation and design notes.');
          }
          await dialog
            .getByRole('button', { name: 'Next', exact: true })
            .click();
          await expect(dialog.locator('[aria-current="step"]')).toContainText(
            '3'
          );
          await expect(
            dialog.getByRole('button', { name: 'Previous', exact: true })
          ).toBeVisible();
          if (wizard.name === 'wiki') {
            await dialog
              .getByLabel('Title', { exact: true })
              .fill('Design notes');
            await dialog.locator('#knowledge-tags').fill('design');
            await dialog.locator('#knowledge-tags').press('Enter');
            await expect(dialog.locator('.tag-item')).toBeVisible();
          }
          if (wizard.name === 'tasks') {
            const priorities = await dialog
              .locator('.priority-selector')
              .evaluate((element) => ({
                content: element.scrollWidth,
                width: element.clientWidth,
              }));
            expect
              .soft(priorities.content)
              .toBeLessThanOrEqual(priorities.width);
            const toggle = await dialog
              .locator('.toggle-wrapper')
              .first()
              .boundingBox();
            expect(toggle).not.toBeNull();
            expect.soft(toggle!.width).toBeCloseTo(32, 3);
            expect.soft(toggle!.height).toBeCloseTo(18, 3);
            // Named, because enabling agentic mode reveals a second toggle
            // (plan approval) inside the same dialog.
            const agentic = dialog.getByRole('checkbox', {
              name: /Enable Agentic Mode/,
            });
            const planApproval = dialog.getByText('Require plan approval', {
              exact: true,
            });
            await expect(planApproval).toHaveCount(0);
            await dialog
              .getByText('Enable Agentic Mode', { exact: true })
              .click();
            await expect(agentic).toBeChecked();
            await expect(planApproval).toBeVisible();
            await agentic.focus();
            await agentic.press('Space');
            await expect(agentic).not.toBeChecked();
            await expect(planApproval).toHaveCount(0);
          }
          const footer = await dialog.locator('footer').boundingBox();
          expect(footer!.y + footer!.height).toBeLessThanOrEqual(
            viewport.height
          );
          await capture(
            page,
            `${profile}-${wizard.name}-wizard-details`,
            theme
          );
        });
      }

      test('conversation uses a full reading pane and preserves markdown hierarchy', async ({
        page,
      }) => {
        await routeApi(page, '**/api/chats/chat-1', (route) =>
          route.fulfill({
            json: {
              chat: {
                ...mockChats[0],
                messages: [
                  {
                    id: 'message-1',
                    chat_id: 'chat-1',
                    role: 'user',
                    content:
                      'Please review the API design and summarize the next steps.',
                    created_at: '2024-01-15T12:00:00Z',
                  },
                  {
                    id: 'message-2',
                    chat_id: 'chat-1',
                    role: 'assistant',
                    content:
                      '# API review\n\nThe implementation follows the workspace conventions.\n\n## Next steps\n\n- Document the response format.\n- Add a regression test.\n\n```typescript\nconst response = await fetch("/api/projects");\n```',
                    created_at: '2024-01-15T12:01:00Z',
                    metadata: {
                      tool_calls: [
                        {
                          id: 'tool-1',
                          name: 'search_workspace',
                          arguments: '{"query":"API design"}',
                          success: true,
                          detail: 'Found two design documents',
                          duration_ms: 85,
                        },
                      ],
                    },
                  },
                ],
              },
            },
          })
        );
        await page.routeWebSocket('**/ws/chats/**', (socket) => {
          socket.onMessage(() =>
            socket.send(JSON.stringify({ type: 'authenticated' }))
          );
        });
        await ready(page, '/chats', theme);
        await page.locator('.chat-item').first().click();
        await expect(page.locator('.message-assistant')).toBeVisible();
        await expect(page.locator('.message')).toHaveCount(2);
        await expect(page.locator('.message-assistant')).toHaveCSS(
          'font-size',
          '14px'
        );
        await expect(page.locator('.message-assistant')).toHaveCSS(
          'line-height',
          '22px'
        );
        await expect(page.locator('.message-form textarea')).toBeVisible();
        await expect(page.locator('.tool-call-summary')).toBeVisible();
        await fitsViewport(page);
        await page.locator('.tool-call-summary').click();
        await expect(page.locator('.tool-call-args')).toBeVisible();
        await page.locator('input[type="file"]').setInputFiles({
          name: 'design-notes.txt',
          mimeType: 'text/plain',
          buffer: Buffer.from(
            'Use the same spacing scale across the workspace.'
          ),
        });
        await expect(page.locator('.attachment-chip')).toBeVisible();
        await expect(
          page.getByText('Chat connection failed')
        ).not.toBeVisible();
        await capture(page, `${profile}-conversation`, theme);
        if (viewport.width < 768) {
          await expect(page.locator('.chats-sidebar')).not.toBeVisible();
          const reading = await page.locator('.chats-main').boundingBox();
          expect(reading!.width).toBeGreaterThanOrEqual(viewport.width - 32);
          await page.getByRole('button', { name: 'Back to chats' }).click();
          await expect(page.locator('.chat-item').first()).toBeVisible();
          await routeApi(page, '**/api/chats/chat-1', (route) =>
            route.fulfill({ status: 500, json: { error: 'Unavailable' } })
          );
          await page.reload();
          await page.locator('.chat-item').first().click();
          await expect(page.locator('.chat-header .page-bar-lead-btn')).toBeVisible();
          await page.getByRole('button', { name: 'Back to chats' }).click();
          await expect(page.locator('.chat-item').first()).toBeVisible();
        }
      });

      test('detail panes and search results share the same content rhythm', async ({
        page,
      }) => {
        await ready(page, '/projects', theme);
        await page.locator('.project-card').first().click();
        await expect(page.locator('.project-details')).toBeVisible();
        await expect(page.locator('.details-content')).toHaveCSS(
          'padding-left',
          viewport.width > 768 ? '20px' : '16px'
        );
        await expect(page.locator('.sync-config-section')).toBeVisible();
        await capture(page, `${profile}-project-details`, theme);
        await fitsViewport(page);
        await ready(page, '/wiki', theme);
        await page.locator('.knowledge-card').first().click();
        await expect(
          page.getByRole('dialog', { name: 'Getting Started Guide' })
        ).toBeVisible();
        await expect(page.locator('.wiki-dialog-content')).toHaveCSS(
          'font-size',
          '14px'
        );
        await capture(page, `${profile}-wiki-details`, theme);
        await fitsViewport(page);
        await ready(page, '/tasks', theme);
        await page
          .getByRole('button', {
            name: 'Actions for Implement authentication',
            exact: true,
          })
          .click();
        await page.getByRole('menuitem', { name: 'Run', exact: true }).click();
        await expect(
          page.getByText('Ready to run', { exact: true })
        ).toBeVisible();
        await expect(page.getByRole('dialog').getByRole('heading')).toHaveCSS(
          'font-size',
          '18px'
        );
        await capture(page, `${profile}-execution`, theme);
        await fitsViewport(page);
        await routeApi(page, /\/api\/context\/search/, (route) =>
          route.fulfill({
            json: {
              total: 1,
              results: [
                {
                  id: 'result-1',
                  source_id: 'source-1',
                  source_name: 'API design guide',
                  content: 'Use consistent workspace typography and spacing.',
                  snippet: 'Use consistent workspace typography and spacing.',
                  relevance_score: 0.95,
                  metadata: { type: 'file', path: '/docs/design-guide.md' },
                },
              ],
            },
          })
        );
        await ready(page, '/wiki', theme);
        await page
          .getByPlaceholder('Search knowledge and sources...')
          .fill('design');
        await page.getByRole('button', { name: 'Search', exact: true }).click();
        await expect(page.locator('.result-card')).toBeVisible();
        await capture(page, `${profile}-search-results`, theme);
        await fitsViewport(page);
      });

      test('browse rows grow to contain wrapped metadata without overlapping', async ({
        page,
      }) => {
        await routeApi(page, /\/api\/models\?/, (route) =>
          route.fulfill({
            json: {
              models: Array.from({ length: 24 }, (_, index) => ({
                name: `review-model-${index}:latest`,
                display_name: `Workspace review model ${index + 1}`,
                source: 'ollama',
                description:
                  'A general purpose model for reasoning, coding, and reviewing long workspace documentation with detailed context.',
                size: 4700000000,
                downloads: 120000,
                capabilities: ['text', 'tools', 'image_input'],
                details: {
                  family: 'llama',
                  parameter_size: '8B',
                  quantization_level: 'Q4_K_M',
                  context_length: 128000,
                },
              })),
              next_cursor: null,
            },
          })
        );
        await ready(page, '/models', theme);
        await page.getByRole('tab', { name: /Browse/ }).click();
        await expect(page.locator('.browse-item').first()).toBeVisible();
        await expect(page.locator('.browse-description').first()).toBeVisible();
        await expect
          .soft(
            page
              .locator('.browse-item')
              .first()
              .getByRole('button', { name: 'Install', exact: true })
          )
          .toHaveCSS('font-size', '12px');
        const rows = await page
          .locator('.virtual-browse-item-wrapper')
          .evaluateAll((elements) =>
            elements.slice(0, 3).map((element) => {
              const row = element.getBoundingClientRect();
              const card = element
                .querySelector('.browse-item')!
                .getBoundingClientRect();
              return {
                top: row.top,
                bottom: row.bottom,
                cardBottom: card.bottom,
              };
            })
          );
        expect(rows.length).toBeGreaterThan(1);
        for (let index = 0; index < rows.length - 1; index++)
          expect(rows[index].cardBottom).toBeLessThanOrEqual(
            rows[index + 1].top + 1
          );
        await fitsViewport(page);
        await capture(page, `${profile}-browse`, theme);
        await page.locator('.browse-item').first().click();
        const details = page.getByRole('dialog');
        await expect(details).toBeVisible();
        await capture(page, `${profile}-model-details`, theme);
        if (viewport.width === 390) {
          await expect
            .poll(async () => {
              const bounds = await details.boundingBox();
              return bounds ? Math.round(bounds.x) : null;
            })
            .toBe(16);
          const bounds = await details.boundingBox();
          expect(
            bounds ? Math.round(viewport.width - bounds.x - bounds.width) : null
          ).toBe(16);
        }
      });
    });
  }
}

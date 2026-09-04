import type { Page } from '@playwright/test';
import { expect, test } from './fixtures';
import {
  mockChats,
  setupAdminAuth,
  setupCommonRoutes,
} from './layout-fixtures';
import { blockServiceWorker, routeApi } from './test-utils';

const screens = [
  {
    path: 'chats',
    heading: 'Chats',
    content: '.chat-item',
    title: '.chat-title',
  },
  {
    path: 'projects',
    heading: 'Projects',
    content: '.project-card',
    title: '.project-name',
  },
  {
    path: 'tasks',
    heading: 'Tasks',
    content: '.task-card:not(.skeleton-card)',
    title: '.task-card-header h3',
  },
  {
    path: 'sources',
    heading: 'Sources',
    content: '.source-card',
    title: '.source-card-header h3',
  },
  {
    path: 'wiki',
    heading: 'Knowledge Base',
    content: '.knowledge-card',
    title: '.knowledge-card-title',
  },
  {
    path: 'models',
    heading: 'Models',
    content: '.model-item',
    title: '.model-name',
  },
  {
    path: 'search',
    heading: 'Context Search',
    content: '.search-section',
    title: null,
  },
];

async function fitsViewport(page: Page): Promise<void> {
  const width = await page.evaluate(() => ({
    content: document.documentElement.scrollWidth,
    viewport: innerWidth,
  }));
  expect(width.content).toBeLessThanOrEqual(width.viewport);
}

async function capture(page: Page, name: string): Promise<void> {
  if (process.env.RUN_SCREENSHOTS === 'true')
    await page.screenshot({
      path: `screenshots/layout/${name}.png`,
      fullPage: true,
      animations: 'disabled',
    });
}

async function ready(page: Page, path: string, theme: string): Promise<void> {
  await page.goto(`/${path}`, { waitUntil: 'domcontentloaded' });
  await page.evaluate(
    (value) => document.documentElement.setAttribute('data-theme', value),
    theme
  );
  await page.evaluate(() => document.fonts.ready);
}

for (const viewport of [
  { width: 1440, height: 1000 },
  { width: 390, height: 844 },
]) {
  for (const theme of ['light', 'dark']) {
    const profile = `${viewport.width}-${theme}`;
    test.describe(`Workspace layout ${profile}`, () => {
      test.use({ viewport });
      test.beforeEach(async ({ page, context }) => {
        await blockServiceWorker(context);
        await setupCommonRoutes(page, true);
        await page.goto('/login', { waitUntil: 'domcontentloaded' });
        await setupAdminAuth(page);
      });

      test('populated screens share title and list typography without horizontal overflow', async ({
        page,
      }) => {
        for (const screen of screens) {
          await ready(page, screen.path, theme);
          const heading = page.getByRole('heading', {
            name: screen.heading,
            exact: true,
          });
          await expect(heading).toBeVisible();
          await expect(page.locator(screen.content).first()).toBeVisible();
          await expect(page.locator('.skeleton-card')).toHaveCount(0);
          await expect(heading).toHaveCSS('font-size', '20px');
          await expect(heading).toHaveCSS('line-height', '25px');
          if (viewport.width > 768) {
            const header =
              screen.path === 'chats'
                ? '.chats-sidebar-header'
                : screen.path === 'search'
                  ? '.context-search-header'
                  : `.${screen.path}-header`;
            await expect(page.locator(header)).toHaveCSS('height', '56px');
          }
          if (screen.title) {
            await expect(page.locator(screen.title).first()).toHaveCSS(
              'font-size',
              '14px'
            );
            await expect(page.locator(screen.title).first()).toHaveCSS(
              'line-height',
              '21px'
            );
          }
          await fitsViewport(page);
          await capture(page, `${profile}-${screen.path}`);
        }
      });

      test('creation wizards keep their title, steps, content and footer visible', async ({
        page,
      }) => {
        for (const wizard of [
          { path: 'projects', button: '+ New Project', title: 'New Project' },
          { path: 'tasks', button: '+ New Task', title: 'New Task' },
          { path: 'sources', button: '+ Add Source', title: 'Add Source' },
          {
            path: 'wiki',
            button: '+ Add Knowledge',
            title: 'Add Knowledge Entry',
          },
        ]) {
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
          ).toHaveCSS('font-size', '20px');
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
          await capture(page, `${profile}-${wizard.path}-wizard`);
          if (wizard.path === 'projects') {
            await dialog.getByLabel('Project Name').fill('Layout review');
          } else if (wizard.path === 'tasks') {
            await dialog.locator('.project-selection-option').first().click();
          } else if (wizard.path === 'sources') {
            await dialog
              .locator('.source-type-option')
              .filter({ hasText: 'Add raw text content' })
              .click();
          }
          await dialog
            .getByRole('button', { name: 'Next', exact: true })
            .click();
          await expect(dialog.locator('[aria-current="step"]')).toContainText(
            '2'
          );
          await capture(page, `${profile}-${wizard.path}-wizard-content`);
          if (wizard.path === 'tasks') {
            await dialog
              .getByLabel('Title', { exact: true })
              .fill('Review layout');
            await dialog
              .getByLabel('Description', { exact: true })
              .fill('Keep typography and spacing consistent.');
          } else if (wizard.path === 'sources' || wizard.path === 'wiki') {
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
          if (wizard.path === 'wiki') {
            await dialog
              .getByLabel('Title', { exact: true })
              .fill('Design notes');
            await dialog.locator('#knowledge-tags').fill('design');
            await dialog.locator('#knowledge-tags').press('Enter');
            await expect(dialog.locator('.tag-item')).toBeVisible();
          }
          const footer = await dialog.locator('footer').boundingBox();
          expect(footer!.y + footer!.height).toBeLessThanOrEqual(
            viewport.height
          );
          await capture(page, `${profile}-${wizard.path}-wizard-details`);
        }
      });

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
        await ready(page, 'chats', theme);
        await page.locator('.chat-item').first().click();
        await expect(page.locator('.message-assistant')).toBeVisible();
        await expect(page.locator('.message')).toHaveCount(2);
        await expect(page.locator('.message-assistant')).toHaveCSS(
          'font-size',
          '16px'
        );
        await expect(page.locator('.message-assistant')).toHaveCSS(
          'line-height',
          '24px'
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
        await capture(page, `${profile}-conversation`);
        if (viewport.width < 768) {
          await expect(page.locator('.chats-sidebar')).not.toBeVisible();
          const reading = await page.locator('.chats-main').boundingBox();
          expect(reading!.width).toBeGreaterThanOrEqual(viewport.width - 32);
          await page.getByRole('button', { name: 'Back to chats' }).click();
          await expect(page.locator('.chat-item').first()).toBeVisible();
          await routeApi(page, '**/api/chats/chat-1', (route) =>
            route.fulfill({ status: 500, json: { error: 'Unavailable' } })
          );
          await page.locator('.chat-item').first().click();
          await expect(page.locator('.chat-back-state')).toBeVisible();
          await page.getByRole('button', { name: 'Back to chats' }).click();
          await expect(page.locator('.chat-item').first()).toBeVisible();
        }
      });

      test('detail panes and search results share the same content rhythm', async ({
        page,
      }) => {
        await ready(page, 'projects', theme);
        await page.locator('.project-card').first().click();
        await expect(page.locator('.project-details')).toBeVisible();
        await expect(page.locator('.details-content')).toHaveCSS(
          'padding-left',
          viewport.width > 768 ? '24px' : '16px'
        );
        await expect(page.locator('.sync-config-section')).toBeVisible();
        await capture(page, `${profile}-project-details`);
        await fitsViewport(page);
        await ready(page, 'wiki', theme);
        await page.locator('.knowledge-card').first().click();
        await expect(
          page.getByRole('dialog', { name: 'Getting Started Guide' })
        ).toBeVisible();
        await expect(page.locator('.wiki-dialog-content')).toHaveCSS(
          'font-size',
          '16px'
        );
        await capture(page, `${profile}-wiki-details`);
        await fitsViewport(page);
        await ready(page, 'tasks', theme);
        await page
          .getByRole('button', { name: 'Execute', exact: true })
          .first()
          .click();
        await expect(
          page.getByText('Ready to run', { exact: true })
        ).toBeVisible();
        await expect(page.getByRole('dialog').getByRole('heading')).toHaveCSS(
          'font-size',
          '20px'
        );
        await capture(page, `${profile}-execution`);
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
        await ready(page, 'search', theme);
        await page
          .getByPlaceholder('Search your knowledge base...')
          .fill('design');
        await page.getByRole('button', { name: 'Search', exact: true }).click();
        await expect(page.locator('.result-card')).toBeVisible();
        await capture(page, `${profile}-search-results`);
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
        await ready(page, 'models', theme);
        await page.getByRole('tab', { name: /Browse/ }).click();
        await expect(page.locator('.browse-item').first()).toBeVisible();
        await expect(page.locator('.browse-description').first()).toBeVisible();
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
        await capture(page, `${profile}-browse`);
      });
    });
  }
}

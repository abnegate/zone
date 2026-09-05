import type { Page } from '@playwright/test';
import { test, expect } from './fixtures';
import { setupAuth, mockCommonEndpoints } from './helpers/auth';
import { blockServiceWorker, routeApi } from './test-utils';

// Mock data generators
const generateMockChat = (
  id: string,
  title: string,
  modelName: string,
  archived = false,
  options: { agent_enabled?: boolean } = {}
) => ({
  id,
  title,
  model_name: modelName,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived,
  agent_enabled: options.agent_enabled ?? false,
});

const generateMockMessage = (id: string, chatId: string, role: string, content: string) => ({
  id,
  chat_id: chatId,
  role,
  content,
  created_at: new Date().toISOString(),
});

const generateMockModel = (name: string) => ({
  name,
  size: 1024 * 1024 * 100,
  modified_at: new Date().toISOString(),
});

const openNewChatFromSidebar = (page: Page) =>
  page.getByRole('button', { name: 'New chat', exact: true }).click();

const mockOpenChatSocket = async (page: Page, chatId = 'chat-1') => {
  await page.routeWebSocket(new RegExp(`/ws/chats/${chatId}`), (ws) => {
    ws.onMessage((message) => {
      const payload = JSON.parse(typeof message === 'string' ? message : message.toString());
      if (payload.type === 'auth') {
        ws.send(JSON.stringify({ type: 'init', chat_id: chatId, status: 'connected' }));
        return;
      }
      if (payload.type !== 'send') {
        return;
      }
      ws.send(
        JSON.stringify({
          type: 'message_saved',
          message_id: `saved-${Date.now()}`,
          role: 'user',
          content: payload.content,
        })
      );
    });
  });
};

test.describe('Chats Page', () => {
  test.beforeEach(async ({ context, page }) => {
    // Block service worker to allow route interception to work
    await blockServiceWorker(context);
    await mockCommonEndpoints(page);

    // Mock models API for the new chat dialog
    await page.unroute('**/api/models*');
    await routeApi(page, '**/api/models*', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          models: [
            generateMockModel('llama3.2'),
            generateMockModel('codellama'),
            generateMockModel('mistral'),
          ],
        }),
      });
    });

    // Default mock for chats - use regex for better matching
    await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
      if (route.request().method() === 'GET') {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: [] }),
        });
      } else {
        route.continue();
      }
    });

    // Set API key and navigate
    await page.goto('/');
    await setupAuth(page);
    await page.reload();
    await expect(page.locator('.sidebar')).toBeVisible({ timeout: 10000 });

    // Navigate to chats page
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');
  });

  for (const width of [390, 769, 820, 1280]) {
    test(`search text clears the search and clear icons at ${width}px`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 900 });
      const input = page.getByTestId('chat-search-input');
      await input.fill('Search spacing');

      const spacing = await input.evaluate((element) => {
        const field = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        const icon = document.querySelector('.chat-search-icon')!.getBoundingClientRect();
        const clear = document
          .querySelector('.chat-search-clear')!
          .getBoundingClientRect();
        return {
          start: field.left + Number.parseFloat(style.paddingLeft),
          end: field.right - Number.parseFloat(style.paddingRight),
          icon: icon.right,
          clear: clear.left,
        };
      });

      expect(spacing.start).toBeGreaterThan(spacing.icon);
      expect(spacing.end).toBeLessThan(spacing.clear);
      await page.screenshot({ path: test.info().outputPath('chats.png') });
      await page.getByTestId('clear-search-btn').click();
      await expect(input).toHaveValue('');
    });
  }

  test.describe('Empty State', () => {
    test('shows empty state when no chats exist', async ({ page }) => {
      await expect(page.getByText('No chats yet')).toBeVisible();
      await expect(page.getByText('Start a new conversation to get started')).toBeVisible();
    });

    test('shows start new chat button in empty state', async ({ page }) => {
      await expect(page.getByRole('heading', { name: 'No chats yet' })).toBeVisible();
      await expect(
        page.getByRole('button', { name: 'New Chat', exact: true })
      ).toBeVisible();
    });
  });

  test.describe('Chat List', () => {
    test('displays list of chats', async ({ page }) => {
      const mockChats = [
        generateMockChat('chat-1', 'Chat about coding', 'llama3.2'),
        generateMockChat('chat-2', 'Help with math', 'codellama'),
        generateMockChat('chat-3', 'Creative writing', 'mistral'),
      ];

      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        if (route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: mockChats }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      await expect(page.locator('.chat-item')).toHaveCount(3);
      await expect(page.locator('.chat-title').first()).toContainText('Chat about coding');
    });

    test('shows chat model name and timestamp', async ({ page }) => {
      const mockChats = [generateMockChat('chat-1', 'Test Chat', 'llama3.2')];

      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chats: mockChats }),
        });
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      await expect(page.locator('.chat-meta').first()).toContainText('llama3.2');
    });

    test('filters between active and archived chats', async ({ page }) => {
      const activeChats = [generateMockChat('chat-1', 'Active Chat', 'llama3.2', false)];
      const archivedChats = [generateMockChat('chat-2', 'Archived Chat', 'codellama', true)];

      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = new URL(route.request().url());
        const archived = url.searchParams.get('archived');

        if (archived === 'true') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: archivedChats }),
          });
        } else {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: activeChats }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      // Check active chats
      const activeTab = page.getByRole('tab', { name: 'Active' });
      const archivedTab = page.getByRole('tab', { name: 'Archived' });
      await expect(activeTab).toHaveAttribute('data-state', 'active');
      await expect(page.locator('.chat-title').first()).toContainText('Active Chat');

      // Switch to archived
      await archivedTab.click();
      await expect(page.locator('.chat-title').first()).toContainText('Archived Chat');
    });

    test('highlights selected chat', async ({ page }) => {
      const mockChats = [
        generateMockChat('chat-1', 'First Chat', 'llama3.2'),
        generateMockChat('chat-2', 'Second Chat', 'codellama'),
      ];

      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = route.request().url();
        if (url.includes('/chat-1') && route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              chat: { ...mockChats[0], messages: [] },
            }),
          });
        } else if (route.request().method() === 'GET' && !url.includes('/chat-')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: mockChats }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      // Click first chat - wait for chat items to be visible first
      await expect(page.locator('.chat-item')).toHaveCount(2);
      await page.click('.chat-item:first-child');
      await expect(page.locator('.chat-item.active')).toBeVisible();
    });
  });

  test.describe('Create Chat', () => {
    test('opens new chat modal from sidebar button', async ({ page }) => {
      await openNewChatFromSidebar(page);
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toBeVisible();
    });

    test('opens new chat modal from placeholder button', async ({ page }) => {
      await page.getByRole('button', { name: 'Start New Chat' }).click();
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toBeVisible();
    });

    test('shows available models in dropdown', async ({ page }) => {
      await openNewChatFromSidebar(page);

      const selectTrigger = page.getByLabel('Select Model');
      await selectTrigger.click();

      const options = page.getByRole('option');
      await expect(options).toHaveCount(3);
      await expect(page.getByRole('option', { name: 'llama3.2' })).toBeVisible();
      await expect(page.getByRole('option', { name: 'codellama' })).toBeVisible();
      await expect(page.getByRole('option', { name: 'mistral' })).toBeVisible();
    });

    test('creates new chat successfully', async ({ page }) => {
      const newChat = generateMockChat('new-chat-id', 'New Chat', 'llama3.2');

      // Unroute existing chats routes and set up new ones
      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = route.request().url();
        const method = route.request().method();

        if (method === 'POST' && !url.includes('/new-chat-id')) {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ chat: { ...newChat, messages: [] } }),
          });
        } else if (url.includes('/new-chat-id') && method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              chat: { ...newChat, messages: [] },
            }),
          });
        } else if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: [] }),
          });
        }
      });

      await openNewChatFromSidebar(page);
      await page.getByLabel('Select Model').click();
      await page.getByRole('option', { name: 'llama3.2' }).click();
      await page.getByRole('dialog', { name: 'New Chat' }).getByRole('button', {
        name: 'Create Chat',
      }).click();

      // Modal should close
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toHaveCount(0);
    });

    test('disables create button when no model selected', async ({ page }) => {
      await openNewChatFromSidebar(page);
      await expect(
        page.getByRole('dialog', { name: 'New Chat' }).getByRole('button', {
          name: 'Create Chat',
        })
      ).toBeDisabled();
    });

    test('closes modal on cancel', async ({ page }) => {
      await openNewChatFromSidebar(page);
      await page
        .getByRole('dialog', { name: 'New Chat' })
        .getByRole('button', { name: 'Cancel' })
        .click();
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toHaveCount(0);
    });
  });

  test.describe('Chat Conversation', () => {
    const mockChat = generateMockChat('chat-1', 'Test Chat', 'llama3.2');
    const mockMessages = [
      generateMockMessage('msg-1', 'chat-1', 'user', 'Hello, how are you?'),
      generateMockMessage('msg-2', 'chat-1', 'assistant', 'I am doing well, thank you!'),
    ];

    test.beforeEach(async ({ page }) => {
      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = route.request().url();
        if (url.includes('/chat-1') && route.request().method() === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              chat: { ...mockChat, messages: mockMessages },
            }),
          });
        } else if (route.request().method() === 'GET' && !url.includes('/chat-')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: [mockChat] }),
          });
        }
      });

      await mockOpenChatSocket(page);
      await page.reload();
      await page.click('a[href="/chats"]');
      // Wait for chat list to load
      await expect(page.locator('.chat-item')).toHaveCount(1);
    });

    test('displays chat header with title and model', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.chat-header h3')).toContainText('Test Chat');
      await expect(page.locator('.chat-model')).toContainText('llama3.2');
    });

    test('displays messages with correct roles', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.message')).toHaveCount(2);
      await expect(page.locator('.message-user')).toContainText('Hello, how are you?');
      await expect(page.locator('.message-assistant')).toContainText('I am doing well, thank you!');
    });

    test('omits speaker labels on user and assistant messages', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.message-user .message-role')).toHaveCount(0);
      await expect(page.locator('.message-assistant .message-role')).toHaveCount(0);
      await expect(page.locator('.message-user .message-time')).toBeVisible();
      await expect(page.locator('.message-assistant .message-time')).toBeVisible();
    });

    test('shows message input form', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.message-form textarea')).toBeVisible();
      await expect(page.locator('.message-form').getByRole('button', { name: 'Send' })).toBeVisible();
    });

    test('sends message successfully', async ({ page }) => {
      const newMessage = generateMockMessage('msg-3', 'chat-1', 'user', 'Test message');

      await routeApi(page, '**/api/chats/chat-1/messages', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ message: newMessage }),
          });
        }
      });

      await page.click('.chat-item');
      await page.fill('.message-form textarea', 'Test message');
      await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

      // New message should appear
      await expect(page.locator('.message')).toHaveCount(3);
    });

    test('disables send button when input is empty', async ({ page }) => {
      await page.click('.chat-item');
      await expect(page.locator('.message-form').getByRole('button', { name: 'Send' })).toBeDisabled();
    });

    test('clears input after sending', async ({ page }) => {
      const newMessage = generateMockMessage('msg-3', 'chat-1', 'user', 'Test');

      await routeApi(page, '**/api/chats/chat-1/messages', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ message: newMessage }),
          });
        }
      });

      await page.click('.chat-item');
      await page.fill('.message-form textarea', 'Test');
      await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

      await expect(page.locator('.message-form textarea')).toHaveValue('');
    });
  });

  test.describe('Chat Actions', () => {
    const mockChat = generateMockChat('chat-1', 'Test Chat', 'llama3.2');

    test.beforeEach(async ({ page }) => {
      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = route.request().url();
        const method = route.request().method();

        if (url.includes('/chat-1') && method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chat: { ...mockChat, messages: [] } }),
          });
        } else if (method === 'GET' && !url.includes('/chat-')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: [mockChat] }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/chats"]');
    });

    test('shows archive button on hover', async ({ page }) => {
      await page.hover('.chat-item');
      await expect(page.locator('.chat-item-actions button[title="Archive"]')).toBeVisible();
    });

    test('archives chat', async ({ page }) => {
      await routeApi(page, '**/api/chats/chat-1/archive', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ chat: { ...mockChat, archived: true } }),
        });
      });

      await page.hover('.chat-item');
      await page.click('button[title="Archive"]');

      // Chat should be removed from active list (or API called)
    });

    test('shows delete confirmation modal', async ({ page }) => {
      await page.hover('.chat-item');
      await page.click('button[title="Delete"]');

      const deleteDialog = page.getByRole('dialog', { name: 'Delete Chat' });
      await expect(deleteDialog).toBeVisible();
      await expect(deleteDialog).toContainText('cannot be undone');
    });

    test('deletes chat after confirmation', async ({ page }) => {
      await routeApi(page, '**/api/chats/chat-1', (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        }
      });

      await page.hover('.chat-item');
      await page.click('button[title="Delete"]');
      await page
        .getByRole('dialog', { name: 'Delete Chat' })
        .getByRole('button', { name: 'Delete' })
        .click();

      // Modal should close
      await expect(page.getByRole('dialog', { name: 'Delete Chat' })).toHaveCount(0);
    });

    test('cancels delete', async ({ page }) => {
      await page.hover('.chat-item');
      await page.click('button[title="Delete"]');
      await page
        .getByRole('dialog', { name: 'Delete Chat' })
        .getByRole('button', { name: 'Cancel' })
        .click();

      await expect(page.getByRole('dialog', { name: 'Delete Chat' })).toHaveCount(0);
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading chats fails', async ({ page }) => {
      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ success: false, error: 'Server error' }),
        });
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      await expect(page.locator('.chats-error')).toBeVisible();
    });
  });

  test.describe('Accessibility', () => {
    test('chat items are keyboard navigable', async ({ page }) => {
      const mockChats = [generateMockChat('chat-1', 'Test Chat', 'llama3.2')];

      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        if (route.request().method() === 'GET' && !route.request().url().includes('/chat-')) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: mockChats }),
          });
        }
      });

      await page.reload();
      await page.click('a[href="/chats"]');

      // Chat item should be focusable
      await expect(page.locator('.chat-item')).toHaveAttribute('tabindex', '0');
    });

    test('modals can be closed with escape key', async ({ page }) => {
      await openNewChatFromSidebar(page);
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toBeVisible();

      await page.keyboard.press('Escape');
      await expect(page.getByRole('dialog', { name: 'New Chat' })).toHaveCount(0);
    });
  });

  test.describe('Agentic chat', () => {
    const mockChat = generateMockChat('chat-1', 'Agent Chat', 'llama3.2');

    const mockChatRoutes = async (
      page: Page,
      chat: ReturnType<typeof generateMockChat>,
      messages: ReturnType<typeof generateMockMessage>[] = []
    ) => {
      await page.unroute(/\/api\/chats/);
      await routeApi(page, /\/api\/chats($|\?|\/)/i, (route) => {
        const url = route.request().url();
        const method = route.request().method();

        if (method === 'PUT' && url.includes(`/${chat.id}`)) {
          const body = route.request().postDataJSON() as Record<string, unknown>;
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              chat: { ...chat, ...body, messages },
            }),
          });
          return;
        }

        if (method === 'GET' && url.includes(`/${chat.id}`)) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chat: { ...chat, messages } }),
          });
          return;
        }

        if (method === 'GET') {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({ chats: [chat] }),
          });
        }
      });
    };

    const mockChatSocket = async (page: Page) => {
      await page.routeWebSocket(/\/ws\/chats\//, (ws) => {
        ws.onMessage((message) => {
          const payload = JSON.parse(typeof message === 'string' ? message : message.toString());
          if (payload.type === 'auth') {
            ws.send(JSON.stringify({ type: 'init', chat_id: 'chat-1', status: 'connected' }));
            return;
          }
          if (payload.type !== 'send') {
            return;
          }

          ws.send(
            JSON.stringify({
              type: 'message_saved',
              message_id: 'msg-user',
              role: 'user',
              content: payload.content,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'message_start',
              message_id: 'msg-asst',
              role: 'assistant',
            })
          );
          ws.send(
            JSON.stringify({
              type: 'tool_call',
              message_id: 'msg-asst',
              tool_call_id: 'call_1',
              name: 'list_projects',
              arguments: '{}',
            })
          );
          ws.send(
            JSON.stringify({
              type: 'tool_result',
              message_id: 'msg-asst',
              tool_call_id: 'call_1',
              name: 'list_projects',
              success: true,
              detail: 'This workspace has no projects.',
              duration_ms: 12,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'chunk',
              content: 'There are no projects in this workspace.',
              index: 0,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'message_end',
              message_id: 'msg-asst',
              content: 'There are no projects in this workspace.',
              metadata: {
                tool_calls: [
                  {
                    id: 'call_1',
                    name: 'list_projects',
                    arguments: '{}',
                    success: true,
                    detail: 'This workspace has no projects.',
                    duration_ms: 12,
                  },
                ],
              },
            })
          );
        });
      });
    };

    test('turns on agent mode without a separate access toggle', async ({ page }, testInfo) => {
      await mockChatRoutes(page, mockChat);
      await mockChatSocket(page);
      await page.reload();
      await expect(page.locator('.chat-item')).toHaveCount(1);
      await page.click('.chat-item');

      const agentToggle = page.getByTestId('agent-toggle');
      await expect(agentToggle).toHaveAttribute('aria-pressed', 'false');
      await expect(page.getByTestId('sandbox-toggle')).toHaveCount(0);

      await agentToggle.click();

      await expect(agentToggle).toHaveAttribute('aria-pressed', 'true');
      await expect(page.getByTestId('sandbox-toggle')).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Host access' })).toHaveCount(0);
      await expect(page.locator('.message-form textarea')).toBeVisible();
      await page.screenshot({ path: testInfo.outputPath('agent-header.png'), fullPage: true, animations: 'disabled' });
      await openNewChatFromSidebar(page);
      await page.getByRole('checkbox', { name: 'Agent mode', exact: true }).check();
      await expect(page.getByRole('checkbox', { name: 'Sandboxed' })).toHaveCount(0);
      await expect(page.getByText('run shell commands and read and write server files', { exact: false })).toBeVisible();
      await expect(page.getByRole('button', { name: 'Create Chat', exact: true })).toBeVisible();
      await page.screenshot({ path: testInfo.outputPath('agent-form.png'), fullPage: true, animations: 'disabled' });
    });

    test('sends a message and renders the tool the agent ran', async ({ page }) => {
      const agentChat = generateMockChat('chat-1', 'Agent Chat', 'llama3.2', false, {
        agent_enabled: true,
      });
      await mockChatRoutes(page, agentChat);
      await mockChatSocket(page);
      await page.reload();
      await expect(page.locator('.chat-item')).toHaveCount(1);
      await page.click('.chat-item');

      await expect(page.getByTestId('agent-toggle')).toHaveAttribute('aria-pressed', 'true');
      await page.fill('.message-form textarea', 'What projects are in this workspace?');
      await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

      const trace = page.getByTestId('tool-trace');
      await expect(trace).toBeVisible();
      await expect(trace.getByTestId('tool-call')).toContainText('Listed projects');
      await expect(trace.getByTestId('tool-call')).toContainText(
        'This workspace has no projects.'
      );
      await expect(page.locator('.message-assistant .message-content')).toContainText(
        'There are no projects in this workspace.'
      );
    });

    test('shows a durable receipt for a workspace write and keeps it after reload', async ({
      page,
    }) => {
      const receipt = {
        id: 'call_1',
        action: 'create_task',
        target_type: 'task',
        target_id: 'task-1',
        target_label: 'Ship the billing export',
        actor_id: 'user-1',
        actor_name: 'Alice',
        occurred_at: '2026-09-05T10:47:00.000Z',
        success: true,
        outcome: 'Task created',
        href: '/tasks?id=task-1',
      };
      const agentChat = generateMockChat('chat-1', 'Agent Chat', 'llama3.2', false, {
        agent_enabled: true,
      });
      const saved = [
        generateMockMessage('msg-user', 'chat-1', 'user', 'Create a task for the billing export'),
        {
          ...generateMockMessage('msg-asst', 'chat-1', 'assistant', 'Created the task.'),
          metadata: {
            tool_calls: [
              {
                id: 'call_1',
                name: 'create_task',
                arguments: '{"title":"Ship the billing export"}',
                success: true,
                detail: 'Task created',
                duration_ms: 18,
              },
            ],
            action_receipts: [receipt],
          },
        },
      ];

      await mockChatRoutes(page, agentChat);
      await page.routeWebSocket(/\/ws\/chats\//, (ws) => {
        ws.onMessage((message) => {
          const payload = JSON.parse(typeof message === 'string' ? message : message.toString());
          if (payload.type === 'auth') {
            ws.send(JSON.stringify({ type: 'init', chat_id: 'chat-1', status: 'connected' }));
            return;
          }
          if (payload.type !== 'send') {
            return;
          }
          ws.send(
            JSON.stringify({
              type: 'message_saved',
              message_id: 'msg-user',
              role: 'user',
              content: payload.content,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'message_start',
              message_id: 'msg-asst',
              role: 'assistant',
            })
          );
          ws.send(
            JSON.stringify({
              type: 'tool_call',
              message_id: 'msg-asst',
              tool_call_id: 'call_1',
              name: 'create_task',
              arguments: '{"title":"Ship the billing export"}',
            })
          );
          ws.send(
            JSON.stringify({
              type: 'tool_result',
              message_id: 'msg-asst',
              tool_call_id: 'call_1',
              name: 'create_task',
              success: true,
              detail: 'Task created',
              duration_ms: 18,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'action_receipt',
              message_id: 'msg-asst',
              receipt,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'chunk',
              content: 'Created the task.',
              index: 0,
            })
          );
          ws.send(
            JSON.stringify({
              type: 'message_end',
              message_id: 'msg-asst',
              content: 'Created the task.',
              metadata: {
                tool_calls: [
                  {
                    id: 'call_1',
                    name: 'create_task',
                    arguments: '{"title":"Ship the billing export"}',
                    success: true,
                    detail: 'Task created',
                    duration_ms: 18,
                  },
                ],
                action_receipts: [receipt],
              },
            })
          );
        });
      });
      await page.reload();
      await expect(page.locator('.chat-item')).toHaveCount(1);
      await page.click('.chat-item');
      await page.fill('.message-form textarea', 'Create a task for the billing export');
      await page.locator('.message-form').getByRole('button', { name: 'Send' }).click();

      const card = page.getByTestId('action-receipt');
      await expect(card).toBeVisible();
      await expect(card).toContainText('Created task');
      await expect(card).toContainText('Ship the billing export');
      await expect(card).toContainText('Alice');
      await expect(card).toContainText('Task created');
      await expect(page.getByTestId('action-receipt-link')).toHaveAttribute(
        'href',
        '/tasks?id=task-1'
      );

      await mockChatRoutes(page, agentChat, saved);
      await page.reload();
      await expect(page.locator('.chat-item')).toHaveCount(1);
      await page.click('.chat-item');
      await expect(page.getByTestId('action-receipt')).toBeVisible();
      await expect(page.getByTestId('action-receipt-link')).toHaveAttribute(
        'href',
        '/tasks?id=task-1'
      );
    });
  });
});

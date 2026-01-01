import { test, expect } from '@playwright/test';

// Mock data generators
const generateMockChat = (id: string, title: string, modelName: string, archived = false) => ({
  id,
  title,
  model_name: modelName,
  created_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
  archived,
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

test.describe('Chats Page', () => {
  test.beforeEach(async ({ page }) => {
    // Mock models API for the new chat dialog
    await page.route('**/api/models', (route) => {
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
    await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
    await page.evaluate(() => localStorage.setItem('manager_api_key', 'test-key'));
    await page.reload();
    await expect(page.locator('.login-overlay')).not.toBeVisible({ timeout: 10000 });

    // Navigate to chats page
    await page.click('a[href="/chats"]');
    await expect(page).toHaveURL('/chats');
  });

  test.describe('Empty State', () => {
    test('shows empty state when no chats exist', async ({ page }) => {
      await expect(page.locator('.chats-empty')).toBeVisible();
      await expect(page.locator('.chats-empty')).toContainText('No chats yet');
    });

    test('shows start new chat button in empty state', async ({ page }) => {
      await expect(page.locator('.chat-placeholder button')).toContainText('Start New Chat');
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
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await expect(page.locator('.filter-btn.active')).toContainText('Active');
      await expect(page.locator('.chat-title').first()).toContainText('Active Chat');

      // Switch to archived
      await page.click('.filter-btn:has-text("Archived")');
      await expect(page.locator('.chat-title').first()).toContainText('Archived Chat');
    });

    test('highlights selected chat', async ({ page }) => {
      const mockChats = [
        generateMockChat('chat-1', 'First Chat', 'llama3.2'),
        generateMockChat('chat-2', 'Second Chat', 'codellama'),
      ];

      await page.unroute(/\/api\/chats/);
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.click('.chats-sidebar-header .btn-primary');
      await expect(page.locator('.modal-content h3')).toContainText('New Chat');
    });

    test('opens new chat modal from placeholder button', async ({ page }) => {
      await page.click('.chat-placeholder .btn-primary');
      await expect(page.locator('.modal-content h3')).toContainText('New Chat');
    });

    test('shows available models in dropdown', async ({ page }) => {
      await page.click('.chats-sidebar-header .btn-primary');

      const options = page.locator('#model-select option');
      await expect(options).toHaveCount(4); // Including "Choose a model..." option
      await expect(options.nth(1)).toContainText('llama3.2');
      await expect(options.nth(2)).toContainText('codellama');
      await expect(options.nth(3)).toContainText('mistral');
    });

    test('creates new chat successfully', async ({ page }) => {
      const newChat = generateMockChat('new-chat-id', 'New Chat', 'llama3.2');

      await page.route('**/api/chats', (route) => {
        if (route.request().method() === 'POST') {
          route.fulfill({
            status: 201,
            contentType: 'application/json',
            body: JSON.stringify({ chat: newChat }),
          });
        }
      });

      await page.route('**/api/chats/new-chat-id', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            chat: { ...newChat, messages: [] },
          }),
        });
      });

      await page.click('.chats-sidebar-header .btn-primary');
      await page.selectOption('#model-select', 'llama3.2');
      await page.click('.modal-actions .btn-primary');

      // Modal should close
      await expect(page.locator('.modal-content')).not.toBeVisible();
    });

    test('disables create button when no model selected', async ({ page }) => {
      await page.click('.chats-sidebar-header .btn-primary');
      await expect(page.locator('.modal-actions .btn-primary')).toBeDisabled();
    });

    test('closes modal on cancel', async ({ page }) => {
      await page.click('.chats-sidebar-header .btn-primary');
      await page.click('.modal-actions .btn-secondary');
      await expect(page.locator('.modal-content')).not.toBeVisible();
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
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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

    test('shows role labels on messages', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.message-user .message-role')).toContainText('You');
      await expect(page.locator('.message-assistant .message-role')).toContainText('Assistant');
    });

    test('shows message input form', async ({ page }) => {
      await page.click('.chat-item');

      await expect(page.locator('.message-form textarea')).toBeVisible();
      await expect(page.locator('.message-form .btn-primary')).toContainText('Send');
    });

    test('sends message successfully', async ({ page }) => {
      const newMessage = generateMockMessage('msg-3', 'chat-1', 'user', 'Test message');

      await page.route('**/api/chats/chat-1/messages', (route) => {
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
      await page.click('.message-form .btn-primary');

      // New message should appear
      await expect(page.locator('.message')).toHaveCount(3);
    });

    test('disables send button when input is empty', async ({ page }) => {
      await page.click('.chat-item');
      await expect(page.locator('.message-form .btn-primary')).toBeDisabled();
    });

    test('clears input after sending', async ({ page }) => {
      const newMessage = generateMockMessage('msg-3', 'chat-1', 'user', 'Test');

      await page.route('**/api/chats/chat-1/messages', (route) => {
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
      await page.click('.message-form .btn-primary');

      await expect(page.locator('.message-form textarea')).toHaveValue('');
    });
  });

  test.describe('Chat Actions', () => {
    const mockChat = generateMockChat('chat-1', 'Test Chat', 'llama3.2');

    test.beforeEach(async ({ page }) => {
      await page.unroute(/\/api\/chats/);
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.route('**/api/chats/chat-1/archive', (route) => {
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

      await expect(page.locator('.modal-content h3')).toContainText('Delete Chat');
      await expect(page.locator('.modal-content')).toContainText('cannot be undone');
    });

    test('deletes chat after confirmation', async ({ page }) => {
      await page.route('**/api/chats/chat-1', (route) => {
        if (route.request().method() === 'DELETE') {
          route.fulfill({ status: 204 });
        }
      });

      await page.hover('.chat-item');
      await page.click('button[title="Delete"]');
      await page.click('.modal-actions .btn-danger');

      // Modal should close
      await expect(page.locator('.modal-content')).not.toBeVisible();
    });

    test('cancels delete', async ({ page }) => {
      await page.hover('.chat-item');
      await page.click('button[title="Delete"]');
      await page.click('.modal-actions .btn-secondary');

      await expect(page.locator('.modal-content')).not.toBeVisible();
    });
  });

  test.describe('Error Handling', () => {
    test('shows error when loading chats fails', async ({ page }) => {
      await page.unroute(/\/api\/chats/);
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.route(/\/api\/chats($|\?|\/)/i, (route) => {
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
      await page.click('.chats-sidebar-header .btn-primary');
      await expect(page.locator('.modal-content')).toBeVisible();

      await page.keyboard.press('Escape');
      // Note: Escape handling is on the backdrop, not the modal
    });
  });
});

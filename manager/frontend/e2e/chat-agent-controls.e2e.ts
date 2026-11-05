import type { Locator, Page } from '@playwright/test';
import type { Chat, ChatProviderOffer, ChatProviders } from '../src/features/chats/types';
import { expect, test } from './fixtures';
import { mockCommonEndpoints, setupAuth } from './helpers/auth';
import { installChatSocketMock } from './helpers/chatSocket';
import { blockServiceWorker, routeApi } from './test-utils';

const TIMESTAMP = '2026-11-01T09:00:00Z';
const TITLE = 'Refactor the indexer';

const claudeCode: ChatProviderOffer = {
  provider: 'claude_code',
  label: 'Claude Code',
  usable: true,
  reason: null,
  models: [{ name: 'sonnet', label: 'Sonnet', efforts: ['low', 'medium', 'high'] }],
  efforts: ['low', 'medium', 'high'],
  requires_model: false,
};

const agentChat: Chat = {
  id: 'chat-agent',
  title: TITLE,
  model_name: 'auto',
  created_at: TIMESTAMP,
  updated_at: TIMESTAMP,
  archived: false,
  agent_enabled: false,
  auto_approve: false,
  agent_sandboxed: true,
  reasoning_effort: 'auto',
  provider: 'claude_code',
  provider_model: 'sonnet',
};

async function mockAgentChat(page: Page): Promise<void> {
  const entry: Chat = { ...agentChat };
  const providers: ChatProviders = {
    default: claudeCode,
    selected: { ...claudeCode, model: 'sonnet' },
    providers: [claudeCode],
    cli: true,
  };
  const withMessages = () => ({
    ...entry,
    context: null,
    messages: [
      {
        id: 'welcome',
        chat_id: entry.id,
        role: 'assistant',
        content: 'Ready when you are.',
        created_at: TIMESTAMP,
      },
    ],
  });

  await routeApi(page, /\/api\/models(\?|$)/, (route) => route.fulfill({ json: { models: [] } }));
  await routeApi(page, /\/api\/workspaces\/[^/]+\/chat-providers(\?|$)/, (route) =>
    route.fulfill({ json: { default: claudeCode, providers: [claudeCode] } })
  );
  await routeApi(page, /\/api\/chats($|\?|\/)/i, async (route) => {
    const request = route.request();
    const [, id, action] =
      new URL(request.url()).pathname.match(/^\/api\/chats(?:\/([^/]+))?(?:\/(.+))?$/) ?? [];
    if (!id) return route.fulfill({ json: { chats: [entry] } });
    if (id !== entry.id) return route.fulfill({ status: 404, json: { error: 'Chat not found' } });
    if (action === 'providers') return route.fulfill({ json: providers });
    if (action === 'sources') return route.fulfill({ json: { sources: [] } });
    if (action === 'messages') return route.fulfill({ json: { messages: withMessages().messages } });
    if (action) return route.fulfill({ status: 404, json: { error: 'Not mocked' } });
    if (request.method() === 'PUT') Object.assign(entry, request.postDataJSON() as Partial<Chat>);
    return route.fulfill({ json: { chat: withMessages() } });
  });
}

const GROUP_TEXT = 'button, .agent-permission-chip-label';
const PANEL_TEXT = '.agent-permission-name, .agent-permission-hint, .agent-permission-state';
const AA_TEXT = 4.5;
const AA_GRAPHIC = 3;

async function lowestContrast(scope: Locator, selector: string): Promise<number> {
  return scope.evaluate((root, selector) => {
    type Rgba = [number, number, number, number];
    const SENTINEL = 'rgb(1, 2, 3)';
    const canvas = document.createElement('canvas').getContext('2d', {
      willReadFrequently: true,
    })!;
    const parse = (value: string): Rgba => {
      canvas.fillStyle = SENTINEL;
      canvas.fillStyle = value;
      if (canvas.fillStyle === '#010203' && value !== SENTINEL) {
        throw new Error(`Canvas could not read the color ${value}`);
      }
      canvas.clearRect(0, 0, 1, 1);
      canvas.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = canvas.getImageData(0, 0, 1, 1).data;
      return [r, g, b, a / 255];
    };
    const over = (top: Rgba, bottom: Rgba): Rgba => [
      top[0] * top[3] + bottom[0] * (1 - top[3]),
      top[1] * top[3] + bottom[1] * (1 - top[3]),
      top[2] * top[3] + bottom[2] * (1 - top[3]),
      1,
    ];
    const luminance = ([r, g, b]: Rgba) => {
      const linear = (channel: number) => {
        const value = channel / 255;
        return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    };
    const backdrop = (element: Element): Rgba => {
      const layers: Rgba[] = [];
      for (let node: Element | null = element; node; node = node.parentElement) {
        layers.push(parse(getComputedStyle(node).backgroundColor));
      }
      return layers.reduceRight<Rgba>((below, layer) => over(layer, below), [255, 255, 255, 1]);
    };
    const measured = [...root.querySelectorAll(selector)].filter(
      (element) =>
        element.checkVisibility() &&
        (element instanceof SVGElement || Boolean(element.textContent?.trim()))
    );
    if (measured.length === 0) throw new Error(`Nothing visible matches ${selector}`);
    return Math.min(
      ...measured.map((element) => {
        const background = backdrop(element);
        const foreground = luminance(over(parse(getComputedStyle(element).color), background));
        const behind = luminance(background);
        return (Math.max(foreground, behind) + 0.05) / (Math.min(foreground, behind) + 0.05);
      })
    );
  }, selector);
}

async function closePanel(page: Page, trigger: Locator): Promise<void> {
  await page.keyboard.press('Escape');
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await expect(trigger).toBeFocused();
  await trigger.blur();
}

for (const width of [1280, 375]) {
  for (const theme of ['light', 'dark'] as const) {
    test(`agent controls at ${width}px in ${theme}`, async ({ browserName, context, page }) => {
      await page.setViewportSize({ width, height: 720 });
      await page.emulateMedia({ reducedMotion: 'reduce', colorScheme: theme });
      await blockServiceWorker(context);
      await installChatSocketMock(page);
      await mockCommonEndpoints(page);
      await mockAgentChat(page);
      await page.addInitScript((value) => localStorage.setItem('manager_theme', value), theme);
      await setupAuth(page);

      const header = page.locator('.chat-header');
      const group = page.getByRole('group', { name: 'Agent' });
      const agent = page.getByTestId('agent-toggle');
      const trigger = page.getByTestId('agent-permissions');
      const panel = page.getByRole('dialog', { name: 'Agent permissions' });
      const autoApprove = page.getByTestId('auto-approve-toggle');
      const sandbox = page.getByTestId('agent-sandbox-toggle');
      const chips = trigger.locator('.agent-permission-chip');

      const shoot = async (state: string) => {
        expect(
          await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
          'the page never scrolls sideways'
        ).toBe(true);
        const box = await group.boundingBox();
        expect(box, 'the agent controls are on screen').not.toBeNull();
        expect(box!.x, 'the agent controls start inside the viewport').toBeGreaterThanOrEqual(0);
        expect(box!.x + box!.width, 'the agent controls fit the viewport').toBeLessThanOrEqual(
          width
        );
        expect(box!.height, 'the agent controls stay on one line').toBeLessThanOrEqual(30);
        expect(
          await lowestContrast(group, GROUP_TEXT),
          'every label meets WCAG AA'
        ).toBeGreaterThanOrEqual(AA_TEXT);
        expect(
          await lowestContrast(group, 'svg'),
          'every icon meets WCAG AA for graphics'
        ).toBeGreaterThanOrEqual(AA_GRAPHIC);
        if (await panel.isVisible()) {
          expect(
            await lowestContrast(panel, PANEL_TEXT),
            'every permission meets WCAG AA'
          ).toBeGreaterThanOrEqual(AA_TEXT);
        }
        if (browserName !== 'chromium') return;
        const top = (await header.boundingBox())!;
        const open = (await panel.isVisible()) ? await panel.boundingBox() : null;
        const bottom = Math.max(top.y + top.height, open ? open.y + open.height + 8 : 0);
        await page.screenshot({
          path: `screenshots/chats-agent-${theme}-${width}-${state}.png`,
          clip: { x: top.x, y: top.y, width: top.width, height: bottom - top.y },
          animations: 'disabled',
        });
      };

      await page.goto(`/chats?id=${agentChat.id}`);
      await expect(page.getByRole('heading', { name: TITLE })).toBeVisible();
      await page.mouse.move(0, 0);
      await expect(agent).toHaveAttribute('aria-pressed', 'false');
      await expect(trigger).toHaveCount(0);
      await shoot('off');

      await agent.click();
      await expect(agent).toHaveAttribute('aria-pressed', 'true');
      await expect(trigger).toHaveAttribute('aria-expanded', 'false');
      await expect(chips).toHaveText(['Permissions']);
      await page.mouse.move(0, 0);
      await shoot('on');

      await trigger.click();
      await autoApprove.click();
      await expect(autoApprove).toHaveAttribute('aria-pressed', 'true');
      await closePanel(page, trigger);
      await expect(chips).toHaveText(['Auto-approve']);
      await page.mouse.move(0, 0);
      await shoot('auto-approve');

      await trigger.click();
      await autoApprove.click();
      await sandbox.click();
      await expect(autoApprove).toHaveAttribute('aria-pressed', 'false');
      await expect(sandbox).toHaveAttribute('aria-pressed', 'false');
      await closePanel(page, trigger);
      await expect(chips).toHaveText(['Permissions', 'Host tools']);
      await expect(chips.last()).toHaveClass(/agent-permission-chip--host/);
      await page.mouse.move(0, 0);
      await shoot('host-tools');

      await trigger.click();
      await autoApprove.click();
      await expect(autoApprove).toHaveAttribute('aria-pressed', 'true');
      await closePanel(page, trigger);
      await expect(chips).toHaveText(['Auto-approve', 'Host tools']);
      await page.mouse.move(0, 0);
      await shoot('both');

      await trigger.click();
      await expect(trigger).toHaveAttribute('aria-expanded', 'true');
      await expect(panel).toBeVisible();
      await page.mouse.move(0, 0);
      await shoot('open');
      await closePanel(page, trigger);

      await trigger.hover();
      await shoot('hover');
      await agent.hover();
      await shoot('agent-hover');
      await page.mouse.move(0, 0);
      await page.keyboard.press('Shift');
      await agent.focus();
      await expect(agent).toBeFocused();
      expect(
        await agent.evaluate((element) => element.matches(':focus-visible')),
        'keyboard focus draws the focus ring'
      ).toBe(true);
      await shoot('focus');
    });
  }
}

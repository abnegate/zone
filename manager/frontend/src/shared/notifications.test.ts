import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { notificationsAllowed, notifyAboutChat, registerChatOpener } from './notifications';

const CHAT = 'chat-1';

interface ShownNotification {
  title: string;
  body?: string;
  tag?: string;
  onclick: (() => void) | null;
  closed: boolean;
}

const shown: ShownNotification[] = [];
const requestPermission = mock(async () => 'granted');

class FakeNotification {
  static permission = 'granted';
  static requestPermission = requestPermission;
  onclick: (() => void) | null = null;
  closed = false;

  constructor(
    public title: string,
    options?: { body?: string; tag?: string }
  ) {
    const shownOne = this as unknown as ShownNotification;
    shownOne.body = options?.body;
    shownOne.tag = options?.tag;
    shown.push(shownOne);
  }

  close() {
    this.closed = true;
  }
}

const host = mock();

function asClient(): void {
  (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = { invoke: host };
}

function asBrowser(): void {
  delete (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
}

beforeEach(() => {
  shown.length = 0;
  host.mockReset();
  requestPermission.mockReset();
  FakeNotification.permission = 'granted';
  (globalThis as { Notification?: unknown }).Notification = FakeNotification;
  asBrowser();
});

afterEach(() => {
  asBrowser();
  delete (globalThis as { Notification?: unknown }).Notification;
});

describe('in the desktop and mobile client', () => {
  it('hands the notification to the platform with the chat a tap should open', async () => {
    asClient();
    host.mockResolvedValue(null);

    expect(await notifyAboutChat({ chatId: CHAT, title: 'Zone', body: 'Finished its reply' })).toBe(
      true
    );
    expect(host).toHaveBeenCalledWith('plugin:notification|notify', {
      options: { title: 'Zone', body: 'Finished its reply', extra: { chat: CHAT } },
    });
    expect(shown).toHaveLength(0);
  });

  it('reports a platform that refused to show it', async () => {
    asClient();
    host.mockRejectedValue(new Error('the platform refused'));

    expect(await notifyAboutChat({ chatId: CHAT, title: 'Zone', body: 'Finished' })).toBe(false);
  });

  it('takes the platform at its word when it has already been asked', async () => {
    asClient();
    host.mockResolvedValue(false);

    expect(await notificationsAllowed()).toBe(false);
    expect(host).toHaveBeenCalledWith('plugin:notification|is_permission_granted', undefined);
    expect(host).not.toHaveBeenCalledWith('plugin:notification|request_permission', undefined);
  });

  it('asks the reader when the platform has not been asked, and believes the answer', async () => {
    asClient();
    host.mockImplementation(async (command: string) =>
      command === 'plugin:notification|is_permission_granted' ? null : 'granted'
    );

    expect(await notificationsAllowed()).toBe(true);
    expect(host).toHaveBeenCalledWith('plugin:notification|request_permission', undefined);
  });

  it('stays quiet when the reader refuses the prompt', async () => {
    asClient();
    host.mockImplementation(async (command: string) =>
      command === 'plugin:notification|is_permission_granted' ? null : 'denied'
    );

    expect(await notificationsAllowed()).toBe(false);
  });
});

describe('in a browser', () => {
  it('shows a notification whose click routes to the chat', async () => {
    const opened: string[] = [];
    const undo = registerChatOpener((chatId) => opened.push(chatId));

    expect(
      await notifyAboutChat({
        chatId: CHAT,
        title: 'Push notifications',
        body: 'Finished its reply',
      })
    ).toBe(true);
    expect(shown).toHaveLength(1);
    expect(shown[0]).toMatchObject({
      title: 'Push notifications',
      body: 'Finished its reply',
      tag: CHAT,
    });

    shown[0].onclick?.();

    expect(opened).toEqual([CHAT]);
    expect(shown[0].closed).toBe(true);
    undo();
  });

  it('never prompts for permission, and shows nothing without it', async () => {
    FakeNotification.permission = 'default';

    expect(await notificationsAllowed()).toBe(false);
    expect(await notifyAboutChat({ chatId: CHAT, title: 'Zone', body: 'Finished' })).toBe(false);
    expect(requestPermission).not.toHaveBeenCalled();
    expect(shown).toHaveLength(0);
  });
});

describe('registerChatOpener', () => {
  it('leaves a later opener in place when an earlier one is undone', () => {
    const first: string[] = [];
    const second: string[] = [];
    const undoFirst = registerChatOpener((chatId) => first.push(chatId));
    const undoSecond = registerChatOpener((chatId) => second.push(chatId));

    undoFirst();
    globalThis.__zoneOpenChat?.(CHAT);

    expect(second).toEqual([CHAT]);
    expect(first).toEqual([]);
    undoSecond();
    expect(globalThis.__zoneOpenChat).toBeUndefined();
  });
});

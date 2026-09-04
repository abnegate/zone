import type { Page } from '@playwright/test';

/** 1x1 PNG so image tests need no fixture files. */
const PNG_1X1 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

export const MOCK_PNG_DATA_URL = `data:image/png;base64,${PNG_1X1}`;
export const MOCK_PNG_BYTES = Buffer.from(PNG_1X1, 'base64');

export const GENERATED_ARTIFACT_URL =
  '/api/artifacts/00000000-0000-0000-0000-000000000001/chat-1/msg-generated/generated-image-1.png';

export const generatedAttachment = (url = MOCK_PNG_DATA_URL) => ({
  name: 'generated-image-1.png',
  mime: 'image/png',
  url,
});

export type ChatSocketFrame = Record<string, unknown>;
export type ChatSendPayload = { type: 'send'; content: string; metadata?: unknown };

export type ChatSocketController = {
  setOnSend: (handler: (payload: ChatSendPayload) => Promise<void> | void) => void;
  emit: (frame: ChatSocketFrame) => Promise<void>;
};

/**
 * Replace the chat WebSocket with an in-page mock so e2e can push the same
 * frames the server would, without ComfyUI or a real socket.
 */
export async function installChatSocketMock(page: Page): Promise<ChatSocketController> {
  let onSend: ((payload: ChatSendPayload) => Promise<void> | void) | null = null;

  await page.exposeFunction('__forwardChatSocketMessage', async (raw: string) => {
    let payload: { type?: string };
    try {
      payload = JSON.parse(raw);
    } catch {
      return;
    }
    if (payload.type === 'send' && onSend) {
      await onSend(payload as ChatSendPayload);
    }
  });

  await page.addInitScript(() => {
    const w = window as typeof window & {
      __chatSockets?: unknown[];
      __forwardChatSocketMessage?: (raw: string) => void;
      WebSocket: unknown;
    };
    class MockChatSocket {
      readyState = 1;
      url: string;
      onopen: ((event?: unknown) => void) | null = null;
      onmessage: ((event: { data: string }) => void) | null = null;
      onerror: ((event?: unknown) => void) | null = null;
      onclose: ((event?: unknown) => void) | null = null;
      _listeners: Record<string, Array<(event?: unknown) => void>> = {};

      constructor(url: string) {
        this.url = url;
        w.__chatSockets = w.__chatSockets || [];
        w.__chatSockets.push(this);
        queueMicrotask(() => {
          this.onopen?.();
          (this._listeners.open || []).forEach((fn) => fn());
        });
      }
      send(data: string) {
        w.__forwardChatSocketMessage?.(String(data));
      }
      close() {
        this.readyState = 3;
        w.__chatSockets = (w.__chatSockets || []).filter((socket) => socket !== this);
      }
      addEventListener(type: string, listener: (event?: unknown) => void) {
        this._listeners[type] = this._listeners[type] || [];
        this._listeners[type].push(listener);
      }
      receive(frame: unknown) {
        const event = { data: JSON.stringify(frame) };
        this.onmessage?.(event);
        (this._listeners.message || []).forEach((fn) => fn(event));
      }
    }
    w.WebSocket = MockChatSocket;
  });

  return {
    setOnSend(handler) {
      onSend = handler;
    },
    async emit(frame) {
      await page.evaluate((payload) => {
        const sockets =
          (
            window as Window & {
              __chatSockets?: Array<{ readyState: number; receive: (frame: unknown) => void }>;
            }
          ).__chatSockets ?? [];
        const live = sockets.filter((socket) => socket.readyState === 1);
        live.at(-1)?.receive(payload);
      }, frame);
    },
  };
}

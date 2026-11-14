import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { askClient, inClient } from './client';

const invoke = mock();

function asClient(): void {
  (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = { invoke };
}

function asBrowser(): void {
  delete (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
}

beforeEach(() => {
  invoke.mockReset();
  asBrowser();
});

afterEach(asBrowser);

describe('inClient', () => {
  it('is false in a browser and true inside the client', () => {
    expect(inClient()).toBe(false);
    asClient();
    expect(inClient()).toBe(true);
  });

  it('is false when the host is there but cannot be called', () => {
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    expect(inClient()).toBe(false);
  });
});

describe('askClient', () => {
  it('carries what the host answered, including nothing at all', async () => {
    asClient();
    invoke.mockResolvedValue(null);

    expect(await askClient<null>('notify_chat', { chat: 'chat-1' })).toEqual({
      answered: true,
      value: null,
    });
    expect(invoke).toHaveBeenCalledWith('notify_chat', { chat: 'chat-1' });
  });

  it('reports a browser, where there is no host to ask', async () => {
    expect(await askClient('notify_chat')).toEqual({ answered: false });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('reports a host that refused rather than throwing', async () => {
    asClient();
    invoke.mockRejectedValue(new Error('not a chat id'));

    expect(await askClient('notify_chat')).toEqual({ answered: false });
  });
});

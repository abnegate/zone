import { afterAll, beforeAll, beforeEach, describe, expect, it, jest, mock } from 'bun:test';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ChatActivityReading } from '../types';

const mockGetChatActivity = mock();
const mockNotifyAboutChat = mock();
const mockNotificationsAllowed = mock();

mock.module('../../../api/chats', () => ({
  chatsApi: {
    getChatActivity: mockGetChatActivity,
  },
}));

mock.module('../../../shared/notifications', () => ({
  notificationsAllowed: mockNotificationsAllowed,
  notifyAboutChat: mockNotifyAboutChat,
}));

let useWaitingNotifications: typeof import('./useWaitingNotifications').useWaitingNotifications;
let WATCH_INTERVAL: number;

beforeAll(async () => {
  ({ useWaitingNotifications, WATCH_INTERVAL } = await import('./useWaitingNotifications'));
});

afterAll(() => {
  mock.restore();
});

function reading(chat_id: string, activity: ChatActivityReading['activity']): ChatActivityReading {
  return { chat_id, title: 'Push notifications', activity };
}

/**
 * Drives the watcher's next poll and lets its promises settle.
 *
 * The watcher reads on a timer, so a transition only exists once a second poll
 * has happened; nothing here waits the real twenty seconds for it.
 */
async function nextPoll(): Promise<void> {
  await act(async () => {
    jest.advanceTimersByTime(WATCH_INTERVAL);
    await Promise.resolve();
  });
}

describe('useWaitingNotifications', () => {
  beforeEach(() => {
    mockGetChatActivity.mockReset();
    mockNotifyAboutChat.mockReset();
    mockNotificationsAllowed.mockReset();
    mockNotifyAboutChat.mockResolvedValue(true);
    mockNotificationsAllowed.mockResolvedValue(true);
    document.hasFocus = () => false;
  });

  it('notifies when a chat stops writing, and says what it is waiting for', async () => {
    jest.useFakeTimers();
    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'writing')]);
    renderHook(() => useWaitingNotifications('workspace-1', null));
    await waitFor(() => expect(mockGetChatActivity).toHaveBeenCalledTimes(1));
    expect(mockNotifyAboutChat).not.toHaveBeenCalled();

    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'awaiting_answer')]);
    await nextPoll();

    await waitFor(() =>
      expect(mockNotifyAboutChat).toHaveBeenCalledWith({
        chatId: 'chat-1',
        title: 'Push notifications',
        body: 'Waiting for your answer',
      })
    );
    jest.useRealTimers();
  });

  it('says nothing about a chat that is still writing', async () => {
    jest.useFakeTimers();
    mockGetChatActivity.mockResolvedValue([reading('chat-1', 'writing')]);
    renderHook(() => useWaitingNotifications('workspace-1', null));
    await waitFor(() => expect(mockGetChatActivity).toHaveBeenCalledTimes(1));

    await nextPoll();

    expect(mockNotifyAboutChat).not.toHaveBeenCalled();
    jest.useRealTimers();
  });

  it('leaves the chat the reader is looking at alone, and notifies about it once they look away', async () => {
    jest.useFakeTimers();
    document.hasFocus = () => true;
    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'writing')]);
    renderHook(() => useWaitingNotifications('workspace-1', 'chat-1'));
    await waitFor(() => expect(mockGetChatActivity).toHaveBeenCalledTimes(1));

    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'idle')]);
    await nextPoll();
    expect(mockNotifyAboutChat).not.toHaveBeenCalled();

    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'writing')]);
    await nextPoll();
    document.hasFocus = () => false;
    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'idle')]);
    await nextPoll();

    await waitFor(() => expect(mockNotifyAboutChat).toHaveBeenCalledTimes(1));
    jest.useRealTimers();
  });

  it('reads nothing at all when notifications would not reach the reader', async () => {
    mockNotificationsAllowed.mockResolvedValue(false);
    renderHook(() => useWaitingNotifications('workspace-1', null));

    await waitFor(() => expect(mockNotificationsAllowed).toHaveBeenCalled());
    expect(mockGetChatActivity).not.toHaveBeenCalled();
  });

  it('asks for nothing without a workspace', async () => {
    renderHook(() => useWaitingNotifications(null, null));

    await waitFor(() => expect(mockGetChatActivity).not.toHaveBeenCalled());
    expect(mockNotificationsAllowed).not.toHaveBeenCalled();
  });

  it('still reports the transition after a failed read', async () => {
    jest.useFakeTimers();
    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'writing')]);
    renderHook(() => useWaitingNotifications('workspace-1', null));
    await waitFor(() => expect(mockGetChatActivity).toHaveBeenCalledTimes(1));

    mockGetChatActivity.mockRejectedValueOnce(new Error('the server is unreachable'));
    await nextPoll();
    expect(mockNotifyAboutChat).not.toHaveBeenCalled();

    mockGetChatActivity.mockResolvedValueOnce([reading('chat-1', 'idle')]);
    await nextPoll();

    await waitFor(() => expect(mockNotifyAboutChat).toHaveBeenCalledTimes(1));
    jest.useRealTimers();
  });
});

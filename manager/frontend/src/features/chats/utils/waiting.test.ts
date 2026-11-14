import { describe, expect, it } from 'bun:test';
import type { ChatActivity, ChatActivityReading } from '../types';
import {
  activityByChat,
  UNTITLED_CHAT,
  WAITING_BODIES,
  waitingNotification,
  waitingTransitions,
} from './waiting';

function reading(
  chat_id: string,
  activity: ChatActivity,
  title = 'Report a model host that is down'
): ChatActivityReading {
  return { chat_id, title, activity };
}

describe('waitingTransitions', () => {
  it('reports a chat that finished writing', () => {
    const before: Record<string, ChatActivity> = { 'chat-1': 'writing' };

    expect(waitingTransitions(before, [reading('chat-1', 'idle')])).toEqual([
      {
        chat_id: 'chat-1',
        title: 'Report a model host that is down',
        reason: 'finished',
      },
    ]);
  });

  it('reports a chat that stopped on a question and one that stopped on an approval', () => {
    const before: Record<string, ChatActivity> = { 'chat-1': 'writing', 'chat-2': 'writing' };

    const waiting = waitingTransitions(before, [
      reading('chat-1', 'awaiting_answer'),
      reading('chat-2', 'awaiting_approval'),
    ]);

    expect(waiting.map((chat) => chat.reason)).toEqual(['awaiting_answer', 'awaiting_approval']);
  });

  it('says nothing about a chat that is still writing', () => {
    const before: Record<string, ChatActivity> = { 'chat-1': 'writing' };

    expect(waitingTransitions(before, [reading('chat-1', 'writing')])).toEqual([]);
  });

  it('says nothing about a chat the previous reading had never seen', () => {
    expect(waitingTransitions({}, [reading('chat-1', 'idle')])).toEqual([]);
  });

  it('says nothing about a chat that was already waiting', () => {
    const before: Record<string, ChatActivity> = {
      'chat-1': 'awaiting_answer',
      'chat-2': 'idle',
      'chat-3': 'awaiting_approval',
    };

    const waiting = waitingTransitions(before, [
      reading('chat-1', 'awaiting_approval'),
      reading('chat-2', 'awaiting_answer'),
      reading('chat-3', 'idle'),
    ]);

    expect(waiting).toEqual([]);
  });

  it('names a chat with no title of its own rather than notifying about nothing', () => {
    const before: Record<string, ChatActivity> = { 'chat-1': 'writing' };

    const [waiting] = waitingTransitions(before, [reading('chat-1', 'idle', '   ')]);

    expect(waiting.title).toBe(UNTITLED_CHAT);
  });

  it('reports every chat that stopped in the same poll', () => {
    const before: Record<string, ChatActivity> = {
      'chat-1': 'writing',
      'chat-2': 'writing',
      'chat-3': 'writing',
    };

    const waiting = waitingTransitions(before, [
      reading('chat-1', 'idle'),
      reading('chat-2', 'writing'),
      reading('chat-3', 'awaiting_approval'),
    ]);

    expect(waiting.map((chat) => chat.chat_id)).toEqual(['chat-1', 'chat-3']);
  });
});

describe('activityByChat', () => {
  it('keys the readings by chat so the next poll can compare them', () => {
    expect(activityByChat([reading('chat-1', 'writing'), reading('chat-2', 'idle')])).toEqual({
      'chat-1': 'writing',
      'chat-2': 'idle',
    });
  });
});

describe('waitingNotification', () => {
  it('leads with the chat and says what it is waiting for', () => {
    expect(
      waitingNotification({ chat_id: 'chat-1', title: 'Push notifications', reason: 'finished' })
    ).toEqual({ title: 'Push notifications', body: WAITING_BODIES.finished });
  });

  it('asks for the answer or the approval by name', () => {
    expect(WAITING_BODIES.awaiting_answer).toBe('Waiting for your answer');
    expect(WAITING_BODIES.awaiting_approval).toBe('Waiting for you to approve a tool call');
  });
});

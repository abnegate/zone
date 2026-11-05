import { describe, expect, it } from 'bun:test';
import type { Message } from '../types';
import { galleryItems } from './gallery';

function message(id: string, role: Message['role'], attachments: Message['metadata']): Message {
  return {
    id,
    chat_id: 'chat-1',
    role,
    content: '',
    created_at: '2026-11-05T10:00:00Z',
    metadata: attachments,
  };
}

const GENERATED = '/api/artifacts/workspace/chat-1/message-2/a.png';

describe('galleryItems', () => {
  it('collects every image, video and audio file sent or made, oldest first', () => {
    const items = galleryItems([
      message('message-1', 'user', {
        attachments: [
          { name: 'photo.jpg', mime: 'image/jpeg', url: 'data:image/jpeg;base64,AA==' },
          { name: 'notes.txt', mime: 'text/plain', url: 'data:text/plain;base64,AA==' },
        ],
      }),
      message('message-2', 'assistant', {
        attachments: [
          { name: 'generated-image-1.png', mime: 'image/png', url: GENERATED },
          { name: '', mime: 'video/mp4', url: '/api/artifacts/workspace/chat-1/chat-1/b.mp4' },
          { name: 'speech.mp3', mime: 'audio/mpeg', url: '/api/artifacts/workspace/chat-1/c.mp3' },
          { name: 'empty.png', mime: 'image/png', url: '' },
        ],
      }),
      message('message-3', 'assistant', undefined),
    ]);

    expect(items.map((item) => [item.messageId, item.name, item.kind])).toEqual([
      ['message-1', 'photo.jpg', 'image'],
      ['message-2', 'generated-image-1.png', 'image'],
      ['message-2', 'video 3', 'video'],
      ['message-2', 'speech.mp3', 'audio'],
    ]);
    expect(new Set(items.map((item) => item.key)).size).toBe(items.length);
  });

  it('keeps a file attached again, as a starting image is, where it first appeared', () => {
    const items = galleryItems([
      message('message-2', 'assistant', {
        attachments: [{ name: 'generated-image-1.png', mime: 'image/png', url: GENERATED }],
      }),
      message('message-4', 'user', {
        attachments: [{ name: 'generated-image-1.png', mime: 'image/png', url: GENERATED }],
      }),
    ]);

    expect(items.map((item) => item.messageId)).toEqual(['message-2']);
  });
});

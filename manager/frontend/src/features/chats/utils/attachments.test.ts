import { describe, expect, it } from 'bun:test';
import {
  attachmentMetadata,
  imageAttachments,
  isSendable,
  isStartingImage,
  sourceAttachment,
} from './attachments';

describe('imageAttachments', () => {
  it('returns only image attachments with a url', () => {
    expect(
      imageAttachments({
        attachments: [
          { name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' },
          { name: 'notes.md', mime: 'text/markdown', url: 'https://example.test/notes.md' },
          { name: 'empty.png', mime: 'image/png', url: '' },
        ],
      })
    ).toEqual([{ name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' }]);
  });

  it('returns an empty list when metadata is missing', () => {
    expect(imageAttachments(undefined)).toEqual([]);
    expect(imageAttachments(null)).toEqual([]);
  });
});

describe('attachmentMetadata', () => {
  it('keeps image data URLs for the message payload', () => {
    expect(
      attachmentMetadata([
        {
          id: '1',
          name: 'shot.png',
          size: 12,
          type: 'image/png',
          url: 'data:image/png;base64,xx',
        },
      ])
    ).toEqual({
      attachments: [{ name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' }],
    });
  });
});

describe('isSendable', () => {
  it('accepts images with a data URL', () => {
    expect(
      isSendable({
        id: '1',
        name: 'shot.png',
        size: 12,
        type: 'image/png',
        url: 'data:image/png;base64,xx',
      })
    ).toBe(true);
  });
});

describe('sourceAttachment', () => {
  it('marks a thread image as the next starting image', () => {
    const attachment = sourceAttachment({
      name: 'generated-image-1.png',
      mime: 'image/png',
      url: '/api/artifacts/ws/chat/msg/generated-image-1.png',
    });
    expect(attachment).toEqual({
      id: 'source:/api/artifacts/ws/chat/msg/generated-image-1.png',
      name: 'generated-image-1.png',
      size: 0,
      type: 'image/png',
      url: '/api/artifacts/ws/chat/msg/generated-image-1.png',
      source: true,
    });
    expect(isStartingImage(attachment)).toBe(true);
    expect(isSendable(attachment)).toBe(true);
  });
});

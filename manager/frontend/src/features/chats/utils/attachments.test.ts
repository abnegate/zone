import { describe, expect, it } from 'bun:test';
import {
  attachmentMetadata,
  audioAttachments,
  imageAttachments,
  isSendable,
  isStartingImage,
  sourceAttachment,
  videoAttachments,
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

describe('videoAttachments', () => {
  it('returns only video attachments with a url', () => {
    expect(
      videoAttachments({
        attachments: [
          { name: 'clip.webm', mime: 'video/webm', url: '/api/artifacts/ws/chat/msg/clip.webm' },
          { name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' },
          { name: 'empty.webm', mime: 'video/webm', url: '' },
        ],
      })
    ).toEqual([
      { name: 'clip.webm', mime: 'video/webm', url: '/api/artifacts/ws/chat/msg/clip.webm' },
    ]);
  });

  it('returns an empty list when metadata is missing', () => {
    expect(videoAttachments(undefined)).toEqual([]);
    expect(videoAttachments(null)).toEqual([]);
  });
});

describe('audioAttachments', () => {
  it('returns only audio attachments with a url', () => {
    expect(
      audioAttachments({
        attachments: [
          {
            name: 'generated-audio-1.flac',
            mime: 'audio/flac',
            url: '/api/artifacts/ws/chat/msg/generated-audio-1.flac',
          },
          { name: 'clip.webm', mime: 'video/webm', url: '/api/artifacts/ws/chat/msg/clip.webm' },
          { name: 'empty.flac', mime: 'audio/flac', url: '' },
        ],
      })
    ).toEqual([
      {
        name: 'generated-audio-1.flac',
        mime: 'audio/flac',
        url: '/api/artifacts/ws/chat/msg/generated-audio-1.flac',
      },
    ]);
  });

  it('keeps every generated audio mime the server emits', () => {
    const attachments = [
      { name: 'track.flac', mime: 'audio/flac', url: '/api/artifacts/ws/chat/msg/track.flac' },
      { name: 'track.mp3', mime: 'audio/mpeg', url: '/api/artifacts/ws/chat/msg/track.mp3' },
      { name: 'track.opus', mime: 'audio/opus', url: '/api/artifacts/ws/chat/msg/track.opus' },
      { name: 'track.wav', mime: 'audio/wav', url: '/api/artifacts/ws/chat/msg/track.wav' },
    ];
    expect(audioAttachments({ attachments })).toEqual(attachments);
  });

  it('returns an empty list when metadata is missing', () => {
    expect(audioAttachments(undefined)).toEqual([]);
    expect(audioAttachments(null)).toEqual([]);
  });
});

describe('attachment partitioning', () => {
  it('splits a mixed list across image, video and audio without overlap', () => {
    const image = { name: 'shot.png', mime: 'image/png', url: 'data:image/png;base64,xx' };
    const video = { name: 'clip.webm', mime: 'video/webm', url: '/api/artifacts/clip.webm' };
    const audio = { name: 'track.flac', mime: 'audio/flac', url: '/api/artifacts/track.flac' };
    const notes = { name: 'notes.md', mime: 'text/markdown', url: 'https://example.test/notes.md' };
    const metadata = { attachments: [image, video, audio, notes] };

    expect(imageAttachments(metadata)).toEqual([image]);
    expect(videoAttachments(metadata)).toEqual([video]);
    expect(audioAttachments(metadata)).toEqual([audio]);

    const partitioned = [
      ...imageAttachments(metadata),
      ...videoAttachments(metadata),
      ...audioAttachments(metadata),
    ];
    expect(partitioned).toHaveLength(3);
    expect(new Set(partitioned.map((a) => a.url)).size).toBe(3);
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

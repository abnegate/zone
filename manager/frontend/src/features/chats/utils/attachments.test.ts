import { describe, expect, it } from 'bun:test';
import {
  attachmentMetadata,
  audioAttachments,
  dataUrlBytes,
  imageAttachments,
  isHeicFile,
  isSendable,
  isStartingImage,
  jpegName,
  MAX_IMAGE_BYTES,
  MAX_SOURCE_IMAGE_BYTES,
  readAttachment,
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
      { name: 'track.opus', mime: 'audio/ogg', url: '/api/artifacts/ws/chat/msg/track.opus' },
      { name: 'track.wav', mime: 'audio/wav', url: '/api/artifacts/ws/chat/msg/track.wav' },
    ];
    expect(audioAttachments({ attachments })).toEqual(attachments);
  });

  it('keeps the audio/opus mime stored on messages from before the ogg fix', () => {
    const attachments = [
      { name: 'track.opus', mime: 'audio/opus', url: '/api/artifacts/ws/chat/msg/track.opus' },
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

describe('HEIC attachments', () => {
  const JPEG = 'data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ==';

  function heic(name: string, type: string, size = 2_000_000): File {
    const file = new File([new Uint8Array(0)], name, { type });
    Object.defineProperty(file, 'size', { value: size });
    return file;
  }

  it('recognises a HEIC by name when the platform gives it no type', () => {
    expect(isHeicFile({ name: 'IMG_0001.HEIC', type: '' })).toBe(true);
    expect(isHeicFile({ name: 'IMG_0001.heif', type: 'application/octet-stream' })).toBe(true);
    expect(isHeicFile({ name: 'IMG_0001.jpg', type: 'image/heic' })).toBe(true);
    expect(isHeicFile({ name: 'shot.png', type: 'image/png' })).toBe(false);
    expect(isHeicFile({ name: 'notes.heic.md', type: 'text/markdown' })).toBe(false);
  });

  it('attaches a HEIC as the JPEG the model and the browser can both read', async () => {
    const attachment = await readAttachment(heic('IMG_0001.HEIC', 'image/heic'), {
      transcode: async () => JPEG,
    });
    expect(attachment.url).toBe(JPEG);
    expect(attachment.type).toBe('image/jpeg');
    expect(attachment.name).toBe('IMG_0001.jpg');
    expect(attachment.size).toBe(dataUrlBytes(JPEG));
    expect(attachment.rejected).toBeUndefined();
    expect(isSendable(attachment)).toBe(true);
  });

  it('says so when no decoder, native or bundled, can read the file', async () => {
    const attachment = await readAttachment(heic('IMG_0001.heic', ''), {
      transcode: async () => null,
    });
    expect(attachment.rejected).toBe('this HEIC could not be read');
    expect(attachment.url).toBeUndefined();
  });

  it('allows a phone-sized source but caps the data URL it becomes', async () => {
    const big = `data:image/jpeg;base64,${'A'.repeat(Math.ceil((MAX_IMAGE_BYTES + 1) * (4 / 3)))}`;
    expect(
      (
        await readAttachment(heic('IMG_0001.heic', 'image/heic', MAX_IMAGE_BYTES + 1), {
          transcode: async () => big,
        })
      ).rejected
    ).toBe('over 4.0 MB');

    expect(
      (
        await readAttachment(heic('IMG_0001.heic', 'image/heic', MAX_SOURCE_IMAGE_BYTES + 1), {
          transcode: async () => JPEG,
        })
      ).rejected
    ).toBe('over 16.0 MB');
  });

  it('names the transcoded file even when the original had no extension', () => {
    expect(jpegName('IMG_0001.HEIC')).toBe('IMG_0001.jpg');
    expect(jpegName('photo')).toBe('photo.jpg');
  });

  it('measures a data URL by its bytes, not its characters', () => {
    expect(dataUrlBytes('data:image/jpeg;base64,YWFh')).toBe(3);
    expect(dataUrlBytes('data:image/jpeg;base64,YWE=')).toBe(2);
    expect(dataUrlBytes('data:image/jpeg;base64,YQ==')).toBe(1);
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

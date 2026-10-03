import { describe, expect, it } from 'bun:test';
import {
  accepts,
  fileExtension,
  filesFromDataTransfer,
  isImageFile,
  isVideoFile,
} from './dropFiles';

function file(name: string, type = ''): File {
  return new File(['x'], name, { type });
}

describe('dropFiles', () => {
  it('classifies by MIME and by extension when type is empty', () => {
    expect(isImageFile(file('shot.png', 'image/png'))).toBe(true);
    expect(isImageFile(file('shot.PNG'))).toBe(true);
    expect(isImageFile(file('shot.jpg'))).toBe(true);
    expect(isImageFile(file('notes.txt'))).toBe(false);
    expect(isVideoFile(file('clip.mp4', 'video/mp4'))).toBe(true);
    expect(isVideoFile(file('clip.MOV'))).toBe(true);
    expect(isVideoFile(file('shot.png'))).toBe(false);
    expect(fileExtension('frame.0001.WEBP')).toBe('webp');
  });

  it('accepts empty-type images against an image accept list', () => {
    const accept = 'image/png,image/jpeg,image/webp';
    expect(accepts(accept, file('a.png'))).toBe(true);
    expect(accepts(accept, file('a.jpeg'))).toBe(true);
    expect(accepts(accept, file('a.txt'))).toBe(false);
    expect(accepts(accept, file('a.mp4'))).toBe(false);
    expect(accepts('video/*', file('clip.webm'))).toBe(true);
    expect(accepts('video/*', file('clip.mp4', 'video/mp4'))).toBe(true);
  });

  it('falls back to dataTransfer.files when entries are missing', async () => {
    const dropped = await filesFromDataTransfer(
      {
        files: [file('a.png', 'image/png'), file('notes.txt', 'text/plain')],
        items: [],
      } as unknown as DataTransfer,
      'image/png,image/jpeg,image/webp'
    );
    expect(dropped.map((item) => item.name)).toEqual(['a.png']);
  });

  it('keeps photos and clips from a mixed dump', async () => {
    const accept = 'image/png,image/jpeg,image/webp,video/*';
    expect(accepts(accept, file('walk.mp4', 'video/mp4'))).toBe(true);
    const dropped = await filesFromDataTransfer(
      {
        files: [
          file('portrait.png', 'image/png'),
          file('walk.mp4', 'video/mp4'),
          file('notes.txt', 'text/plain'),
        ],
        items: [],
      } as unknown as DataTransfer,
      accept
    );
    expect(dropped.map((item) => item.name)).toEqual(['portrait.png', 'walk.mp4']);
  });

  it('walks a dropped folder through webkit entries', async () => {
    const shot = file('shot.png', 'image/png');
    const clip = file('take.mov');
    const dropped = await filesFromDataTransfer(
      {
        files: [],
        items: [
          {
            webkitGetAsEntry: () => ({
              isFile: false,
              isDirectory: true,
              createReader: () => {
                let sent = false;
                return {
                  readEntries: (success: (entries: unknown[]) => void) => {
                    if (sent) {
                      success([]);
                      return;
                    }
                    sent = true;
                    success([
                      {
                        isFile: true,
                        isDirectory: false,
                        file: (complete: (value: File) => void) => complete(shot),
                      },
                      {
                        isFile: true,
                        isDirectory: false,
                        file: (complete: (value: File) => void) => complete(clip),
                      },
                    ]);
                  },
                };
              },
            }),
          },
        ],
      } as unknown as DataTransfer,
      'image/png,image/jpeg,image/webp,video/*'
    );
    expect(dropped.map((item) => item.name)).toEqual(['shot.png', 'take.mov']);
  });
});

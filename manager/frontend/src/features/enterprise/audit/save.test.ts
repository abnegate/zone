import { afterAll, afterEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { saveDownload } from './save';

const ANCHOR = 'a';

const URL_OF_BLOB = 'blob:audit';

const FILENAME = 'audit-server-2026-11-01-2026-11-15.ndjson';

const created = spyOn(URL, 'createObjectURL').mockReturnValue(URL_OF_BLOB);

const revoked = spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);

const click = mock();

const links: HTMLAnchorElement[] = [];

const createElement = document.createElement.bind(document);

const creating = spyOn(document, 'createElement').mockImplementation(((tag: string) => {
  const element = createElement(tag);
  if (tag === ANCHOR) {
    const link = element as HTMLAnchorElement;
    link.click = () => click(link.isConnected);
    links.push(link);
  }
  return element;
}) as typeof document.createElement);

afterEach(() => {
  created.mockClear();
  revoked.mockClear();
  click.mockReset();
  links.length = 0;
});

afterAll(() => {
  created.mockRestore();
  revoked.mockRestore();
  creating.mockRestore();
});

describe('saveDownload', () => {
  it('clicks a link to the blob under the filename while it is in the page, then cleans up', () => {
    const blob = new Blob(['{}\n']);

    saveDownload({ blob, filename: FILENAME });

    expect(created).toHaveBeenCalledWith(blob);
    expect(click).toHaveBeenCalledWith(true);
    expect(links).toHaveLength(1);
    expect(links[0].download).toBe(FILENAME);
    expect(links[0].getAttribute('href')).toBe(URL_OF_BLOB);
    expect(links[0].isConnected).toBe(false);
    expect(revoked).toHaveBeenCalledWith(URL_OF_BLOB);
  });

  it('revokes the blob and removes the link even when the click throws', () => {
    click.mockImplementation(() => {
      throw new Error('blocked');
    });

    expect(() => saveDownload({ blob: new Blob([]), filename: 'a.csv' })).toThrow('blocked');
    expect(revoked).toHaveBeenCalledWith(URL_OF_BLOB);
    expect(links[0].isConnected).toBe(false);
  });
});

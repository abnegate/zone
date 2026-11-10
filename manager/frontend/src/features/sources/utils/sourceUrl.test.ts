import { describe, expect, it } from 'bun:test';
import { sourceUrl } from './sourceUrl';

const STORED_LINK = 'https://docs.example.com/project';

describe('sourceUrl', () => {
  it('keeps the link the source was stored with', () => {
    expect(sourceUrl('github', { owner: 'acme', repo: 'zone' }, STORED_LINK)).toBe(STORED_LINK);
  });

  it('links a GitHub repository its config names', () => {
    expect(sourceUrl('github', { owner: 'acme', repo: 'zone' }, null)).toBe(
      'https://github.com/acme/zone'
    );
  });

  it('links nothing for a GitHub config naming an empty owner or repository, as the server', () => {
    for (const config of [
      { owner: '', repo: 'zone' },
      { owner: 'acme', repo: '' },
      { owner: '', repo: '' },
    ]) {
      expect(sourceUrl('github', config, null)).toBe('');
    }
  });

  it('links a GitLab project its config names as project, as the server reads that alias', () => {
    expect(
      sourceUrl('gitlab', { project: 'group/project', host: 'https://gitlab.example.com' }, null)
    ).toBe('https://gitlab.example.com/group/project');
  });

  it('links nothing for a GitLab config naming both project and project_id', () => {
    expect(
      sourceUrl('gitlab', { project: 'group/project', project_id: 'group/project' }, undefined)
    ).toBe('');
  });

  it('links a web or calendar source at its url', () => {
    for (const sourceType of ['web', 'ical'] as const) {
      expect(sourceUrl(sourceType, { url: STORED_LINK }, null)).toBe(STORED_LINK);
    }
  });

  it('links nothing for a config that is not an object', () => {
    expect(sourceUrl('github', null, null)).toBe('');
  });
});

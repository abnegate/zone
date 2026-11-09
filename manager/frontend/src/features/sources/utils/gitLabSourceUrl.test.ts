import { describe, expect, it } from 'bun:test';
import { gitLabSourceUrl } from './gitLabSourceUrl';

const PROJECT = 'group/project';
const INSTANCE = 'https://gitlab.example.com';

describe('gitLabSourceUrl', () => {
  it('links the project at the one instance the config names', () => {
    for (const field of ['host', 'base_url']) {
      expect(gitLabSourceUrl({ project_id: PROJECT, [field]: `${INSTANCE}/api/v4` })).toBe(
        `${INSTANCE}/${PROJECT}`
      );
    }
  });

  it('links gitlab.com when the config names no instance, or a blank one', () => {
    for (const config of [
      { project_id: PROJECT },
      { project_id: PROJECT, host: '' },
      { project_id: PROJECT, base_url: '   ' },
      { project_id: PROJECT, host: null },
    ]) {
      expect(gitLabSourceUrl(config)).toBe(`https://gitlab.com/${PROJECT}`);
    }
  });

  it('links nothing for a config naming both host and base_url, as the server reads neither', () => {
    for (const [host, base_url] of [
      [INSTANCE, INSTANCE],
      [INSTANCE, 'https://other.example.com'],
      ['', INSTANCE],
      [INSTANCE, '  '],
      [null, INSTANCE],
    ]) {
      expect(gitLabSourceUrl({ project_id: PROJECT, host, base_url })).toBeUndefined();
    }
  });

  it('links nothing for an instance named by anything but a string', () => {
    for (const instance of [1, true, { url: INSTANCE }, [INSTANCE]]) {
      expect(gitLabSourceUrl({ project_id: PROJECT, host: instance })).toBeUndefined();
    }
  });

  it('links nothing for a config naming no project', () => {
    for (const config of [{}, { project_id: 42 }, { project_id: '  ' }]) {
      expect(gitLabSourceUrl({ ...config, host: INSTANCE })).toBeUndefined();
    }
  });
});

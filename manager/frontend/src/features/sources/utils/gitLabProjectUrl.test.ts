import { describe, expect, it } from 'bun:test';
import { gitLabProjectUrl } from './gitLabProjectUrl';

const PROJECT = 'group/project';

describe('gitLabProjectUrl', () => {
  it('links the project at its instance web origin, never its API base', () => {
    for (const [instance, origin] of [
      ['https://gitlab.example.com', 'https://gitlab.example.com'],
      ['https://gitlab.example.com/', 'https://gitlab.example.com'],
      ['https://gitlab.example.com/api/v4', 'https://gitlab.example.com'],
      ['https://gitlab.example.com/api/v4/', 'https://gitlab.example.com'],
      [' https://GitLab.example.com:8443/gitlab/api/v4 ', 'https://gitlab.example.com:8443/gitlab'],
      [
        'https://user:secret@gitlab.example.com/?private_token=secret#x',
        'https://gitlab.example.com',
      ],
      ['http://gitlab.example.com', 'http://gitlab.example.com'],
      ['https://gitlab.com/api/v4', 'https://gitlab.com'],
    ]) {
      expect(gitLabProjectUrl(instance, PROJECT)).toBe(`${origin}/${PROJECT}`);
    }
  });

  it('links gitlab.com when no instance is named', () => {
    for (const instance of [undefined, '', '   ']) {
      expect(gitLabProjectUrl(instance, PROJECT)).toBe(`https://gitlab.com/${PROJECT}`);
    }
  });

  it('links nothing for an instance that is no URL', () => {
    expect(gitLabProjectUrl('gitlab.example.com', PROJECT)).toBeUndefined();
  });

  it('links nothing for an instance that is no web page', () => {
    for (const instance of [
      'ftp://gitlab.example.com',
      'javascript:alert(1)',
      'file:///etc/gitlab',
      'mailto:admin@gitlab.example.com',
    ]) {
      expect(gitLabProjectUrl(instance, PROJECT)).toBeUndefined();
    }
  });

  it('links the project it names without the whitespace around it', () => {
    expect(gitLabProjectUrl(undefined, `  ${PROJECT}\n`)).toBe(`https://gitlab.com/${PROJECT}`);
  });

  it('links nothing for a blank project', () => {
    for (const project of ['', '   ']) {
      expect(gitLabProjectUrl(undefined, project)).toBeUndefined();
    }
  });

  it('links a numeric project id at its projects page', () => {
    expect(gitLabProjectUrl('https://gitlab.example.com/api/v4', ' 42 ')).toBe(
      'https://gitlab.example.com/projects/42'
    );
    expect(gitLabProjectUrl(undefined, '42')).toBe('https://gitlab.com/projects/42');
  });
});

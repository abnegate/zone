import { describe, expect, it } from 'bun:test';
import { parseTag, TagProblem, tagExists, tagProblem } from './tags';

describe('tagProblem', () => {
  it.each([
    'v1.2.3',
    '1.2.3',
    'v0.0.0',
    'v1.2.3-rc.1',
    'v10.20.30-beta-2',
    'abnegate-vcs/v0.2.1',
    'crates/abnegate-vcs/0.2.1-rc.1',
    'v18446744073709551615.0.0',
    'pkg x/v1.2.3',
  ])('accepts %s', (tag) => {
    expect(tagProblem(tag)).toBeNull();
  });

  it.each(['', '   '])('asks for a tag when given %j', (tag) => {
    expect(tagProblem(tag)).toBe(TagProblem.Missing);
  });

  it.each([' v1.2.3', 'v1 2 3', 'v1.2.3\t', 'pkg name/v1.2.3'])(
    'refuses whitespace in %j',
    (tag) => {
      expect(tagProblem(tag)).toBe(TagProblem.Whitespace);
    }
  );

  it.each(['v1~1', 'v1^', 'v1:2', 'v1?', 'v1*', 'v1[2]', 'v1\\2', 'v1\u0007', 'v1\u0085'])(
    'refuses the reserved character in %j',
    (tag) => {
      expect(tagProblem(tag)).toBe(TagProblem.Characters);
    }
  );

  it.each(['v1..2', 'pkg..x/v1.2.3', 'v1.2.3-rc..1'])('refuses two dots in a row in %j', (tag) => {
    expect(tagProblem(tag)).toBe(TagProblem.Dots);
  });

  it('refuses the reflog syntax', () => {
    expect(tagProblem('v1@{2}')).toBe(TagProblem.Reflog);
  });

  it.each(['@', 'HEAD'])('refuses the name git reserves, %j', (tag) => {
    expect(tagProblem(tag)).toBe(TagProblem.Reserved);
  });

  it('refuses a leading dash', () => {
    expect(tagProblem('-v1.2.3')).toBe(TagProblem.Dash);
  });

  it('refuses a leading plus', () => {
    expect(tagProblem('+v1.2.3')).toBe(TagProblem.Plus);
  });

  it.each(['/v1.2.3', 'pkg/', 'pkg//v1.2.3'])('refuses the empty path part in %j', (tag) => {
    expect(tagProblem(tag)).toBe(TagProblem.Slashes);
  });

  it.each(['.v1', 'release/.v1.2.3', 'v1.lock', 'v1.lock/v1.2.3'])(
    'refuses the hidden or lock part in %j',
    (tag) => {
      expect(tagProblem(tag)).toBe(TagProblem.Component);
    }
  );

  it('refuses a trailing dot', () => {
    expect(tagProblem('v1.2.')).toBe(TagProblem.Ending);
  });

  it('refuses a tag longer than git allows', () => {
    expect(tagProblem(`${'p'.repeat(250)}/v1.2.3`)).toBe(TagProblem.Length);
    expect(tagProblem(`${'é'.repeat(125)}/v1.2.3`)).toBe(TagProblem.Length);
    expect(tagProblem(`${'p'.repeat(248)}/v1.2.3`)).toBeNull();
  });

  it.each([
    'v',
    'v1',
    'v1.2',
    'v1.2.3.4',
    'v1.2.x',
    'v01.2.3',
    'release-1.2.3',
    'V1.2.3',
    'v1.2.3-',
    'v1.2.3+build',
    'v1.0.0+build.5',
    'pkg/v1.2',
    'release/2026-11-13',
    'v1@2',
    'v18446744073709551616.0.0',
  ])('refuses %j as anything but a three part version', (tag) => {
    expect(tagProblem(tag)).toBe(TagProblem.Version);
  });

  it("refuses a tag the tag's line already has", () => {
    expect(tagProblem('v1.2.3', ['v1.2.2', 'v1.2.3'])).toBe(tagExists('v1.2.3'));
    expect(tagExists('v1.2.3')).toBe('v1.2.3 already exists');
  });

  it('accepts a tag the repository does not have yet', () => {
    expect(tagProblem('v1.2.4', ['v1.2.3'])).toBeNull();
  });

  it('reports a malformed tag before checking whether it exists', () => {
    expect(tagProblem('v1 2', ['v1 2'])).toBe(TagProblem.Whitespace);
  });
});

describe('parseTag', () => {
  it('reads the prefix up to the last slash', () => {
    expect(parseTag('v1.2.3')).toEqual({ prefix: null, prerelease: false });
    expect(parseTag('abnegate-vcs/v0.2.1')).toEqual({ prefix: 'abnegate-vcs', prerelease: false });
    expect(parseTag('crates/abnegate-vcs/0.2.1-rc.1')).toEqual({
      prefix: 'crates/abnegate-vcs',
      prerelease: true,
    });
  });

  it('calls a tag with a pre-release suffix a pre-release', () => {
    expect(parseTag('v1.2.3-rc.1')?.prerelease).toBe(true);
    expect(parseTag('pkg/v1.2.3-beta-2')?.prerelease).toBe(true);
    expect(parseTag('v1.2.3')?.prerelease).toBe(false);
  });

  it('reads nothing from a tag the server would refuse', () => {
    expect(parseTag('v1.2')).toBeNull();
    expect(parseTag('pkg name/v1.2.3')).toBeNull();
    expect(parseTag('')).toBeNull();
  });
});

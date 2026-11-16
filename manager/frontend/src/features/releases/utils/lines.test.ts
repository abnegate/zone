import { describe, expect, it } from 'bun:test';
import { options, prefixedSource } from '../../../test/releases';
import type { SourceOptions } from '../types';
import {
  dominantLine,
  expectedLatestTag,
  lineLabel,
  lineOfTag,
  tagLines,
  UNPREFIXED,
} from './lines';

const plain = options.sources[0];

const untagged: SourceOptions = {
  ...plain,
  latest_tag: null,
  next_tag: 'v0.1.0',
  previous_tags: [],
  lines: [],
};

describe('tagLines', () => {
  it('offers every line the server read', () => {
    expect(tagLines(prefixedSource).map((line) => line.prefix)).toEqual([
      'abnegate-vcs',
      'abnegate-llm',
    ]);
  });

  it('offers the unprefixed first release when the repository has no version tags', () => {
    expect(tagLines(untagged)).toEqual([
      { prefix: null, latest_tag: null, next_tag: 'v0.1.0', previous_tags: [] },
    ]);
  });
});

describe('dominantLine', () => {
  it('picks the line the top-level options describe', () => {
    expect(dominantLine(prefixedSource).prefix).toBe('abnegate-vcs');
    expect(dominantLine({ ...prefixedSource, lines: [...prefixedSource.lines].reverse() })).toBe(
      prefixedSource.lines[0]
    );
  });

  it('falls back to the first line', () => {
    expect(dominantLine({ ...prefixedSource, next_tag: 'other/v1.0.0' })).toBe(
      prefixedSource.lines[0]
    );
  });
});

describe('lineLabel', () => {
  it('names a line by its prefix', () => {
    expect(lineLabel(prefixedSource.lines[1])).toBe('abnegate-llm');
    expect(lineLabel(tagLines(untagged)[0])).toBe(UNPREFIXED);
  });
});

describe('lineOfTag', () => {
  it("finds the tag's own line by its prefix", () => {
    expect(lineOfTag(prefixedSource, 'abnegate-llm/v1.0.1')).toBe(prefixedSource.lines[1]);
    expect(lineOfTag(plain, 'v1.2.4')?.prefix).toBeNull();
  });

  it('finds nothing for a new prefix or a tag that is not a version', () => {
    expect(lineOfTag(prefixedSource, 'abnegate-cli/v0.1.0')).toBeNull();
    expect(lineOfTag(prefixedSource, 'abnegate-llm/next')).toBeNull();
  });
});

describe('expectedLatestTag', () => {
  it("is the latest stable tag of the tag's own line", () => {
    expect(expectedLatestTag(prefixedSource, 'abnegate-llm/v1.0.1')).toBe('abnegate-llm/v1.0.0');
    expect(expectedLatestTag(prefixedSource, 'abnegate-vcs/v0.3.0')).toBe('abnegate-vcs/v0.2.1');
    expect(expectedLatestTag(plain, 'v1.2.4')).toBe('v1.2.3');
  });

  it('is null for a line with no tags yet', () => {
    expect(expectedLatestTag(prefixedSource, 'abnegate-cli/v0.1.0')).toBeNull();
    expect(expectedLatestTag(prefixedSource, 'v0.1.0')).toBeNull();
  });
});

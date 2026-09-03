import { describe, expect, it } from 'bun:test';
import {
  defaultDownloadName,
  formatBytes,
  formatContextLength,
  formatDate,
  formatDownloadSizeLabel,
  formatNumber,
  modelDownloadSizes,
} from './formatters';

describe('formatNumber', () => {
  it('formats thousands and millions', () => {
    expect(formatNumber(950)).toBe('950');
    expect(formatNumber(2800)).toBe('2.8K');
    expect(formatNumber(1_400_000)).toBe('1.4M');
  });
});

describe('formatBytes', () => {
  it('formats byte sizes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(3800000000)).toBe('3.5 GB');
    expect(formatBytes(4100000000)).toBe('3.8 GB');
  });
});

describe('formatContextLength', () => {
  it('formats token windows', () => {
    expect(formatContextLength(262144)).toBe('256K');
    expect(formatContextLength(1048576)).toBe('1M');
    expect(formatContextLength(128000)).toBe('128K');
    expect(formatContextLength(131072)).toBe('128K');
    expect(formatContextLength(512)).toBe('512');
  });
});

describe('formatDate', () => {
  it('returns the original string for invalid dates', () => {
    expect(formatDate('5 days ago')).toBe('5 days ago');
  });
});

describe('modelDownloadSizes', () => {
  it('returns nothing when there is only one size', () => {
    expect(
      modelDownloadSizes({
        name: 'mistral',
        sizes: [{ name: 'mistral:7b', label: '7B' }],
      })
    ).toEqual([]);
  });

  it('returns every size when a model ships more than one', () => {
    const sizes = [
      { name: 'llama3.2:1b', label: '1B', size: 1_300_000_000 },
      { name: 'llama3.2:3b', label: '3B', size: 2_000_000_000 },
    ];
    expect(modelDownloadSizes({ name: 'llama3.2', sizes })).toEqual(sizes);
  });
});

describe('formatDownloadSizeLabel', () => {
  it('includes the download size when known', () => {
    expect(formatDownloadSizeLabel({ name: 'llama3.2:1b', label: '1B', size: 1_300_000_000 })).toBe(
      `1B · ${formatBytes(1_300_000_000)}`
    );
    expect(formatDownloadSizeLabel({ name: 'llama3.2:3b', label: '3B' })).toBe('3B');
  });
});

describe('defaultDownloadName', () => {
  it('uses the smallest listed size, otherwise the model name', () => {
    expect(
      defaultDownloadName({
        name: 'llama3.2',
        sizes: [
          { name: 'llama3.2:1b', label: '1B' },
          { name: 'llama3.2:3b', label: '3B' },
        ],
      })
    ).toBe('llama3.2:1b');
    expect(defaultDownloadName({ name: 'mistral' })).toBe('mistral');
  });
});

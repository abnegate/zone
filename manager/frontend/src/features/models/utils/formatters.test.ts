import { describe, expect, it } from 'bun:test';
import {
  defaultDownloadName,
  downloadOptionRows,
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

describe('downloadOptionRows', () => {
  it('groups GGUF quantizations by bit width', () => {
    const rows = downloadOptionRows([
      { name: 'repo:Q4_0', label: 'Q4_0', size: 4_108_917_024 },
      { name: 'repo:Q5_K_M', label: 'Q5_K_M', size: 5_131_409_696 },
      { name: 'repo:Q8_0', label: 'Q8_0', size: 7_695_857_952 },
    ]);
    expect(rows.map((row) => [row.heading, row.option.label])).toEqual([
      ['4-bit', 'Q4_0'],
      ['5-bit', 'Q5_K_M'],
      ['8-bit', 'Q8_0'],
    ]);
  });

  it('groups mixed parameter GGUF repos by parameter size', () => {
    const rows = downloadOptionRows([
      { name: 'repo:0.6B-Q4_K_M', label: '0.6B · Q4_K_M' },
      { name: 'repo:8B-Q4_K_M', label: '8B · Q4_K_M' },
    ]);
    expect(rows.map((row) => row.heading)).toEqual(['0.6B', '8B']);
  });
});

import type { BrowseModel, ModelSizeOption } from '../types';

/**
 * Format a number with K/M suffix for thousands/millions
 */
export function formatNumber(num: number): string {
  if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
  if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
  return num.toString();
}

export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Number.parseFloat((bytes / k ** i).toFixed(1))} ${sizes[i]}`;
}

const BINARY_CONTEXT_WINDOWS = [
  4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144, 524_288, 1_048_576, 2_097_152, 4_194_304,
  8_388_608,
];

export function formatContextLength(tokens: number): string {
  if (BINARY_CONTEXT_WINDOWS.includes(tokens)) {
    if (tokens >= 1_048_576) return `${tokens / 1_048_576}M`;
    return `${tokens / 1024}K`;
  }
  if (tokens >= 1_000_000 && tokens % 1_000_000 === 0) return `${tokens / 1_000_000}M`;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1000) return `${Math.round(tokens / 1000)}K`;
  return String(tokens);
}

export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  if (Number.isNaN(date.getTime())) return dateStr;
  return date.toLocaleDateString();
}

/** Download options shown in the picker — only when a model ships more than one. */
export function modelDownloadSizes(model: BrowseModel): ModelSizeOption[] {
  const sizes = (model.sizes ?? []).filter((option) => option.name && option.label);
  return sizes.length > 1 ? sizes : [];
}

export function formatDownloadSizeLabel(option: ModelSizeOption): string {
  return option.size ? `${option.label} · ${formatBytes(option.size)}` : option.label;
}

export function defaultDownloadName(model: BrowseModel): string {
  return modelDownloadSizes(model)[0]?.name ?? model.name;
}

const QUANT_TOKEN_RE = /\b((?:IQ|Q)\d+[A-Z0-9_]*|BF16|F16|F32)\b/i;
const PARAM_PREFIX_RE = /^(\d+(?:\.\d+)?[BM])\s·\s(.+)$/i;

export function quantizationFromLabel(label: string): string | null {
  return label.toUpperCase().match(QUANT_TOKEN_RE)?.[1] ?? null;
}

export function quantizationBitLabel(quant: string): string | null {
  const token = quant.toUpperCase();
  if (token === 'F16' || token === 'BF16') return '16-bit';
  if (token === 'F32') return '32-bit';
  const bits = token.match(/(?:IQ|Q)(\d+)/)?.[1];
  return bits ? `${bits}-bit` : null;
}

export type DownloadOptionRow = {
  heading: string | null;
  group: string | null;
  option: ModelSizeOption;
};

/** Group GGUF quants by bit-width (or parameter size) so each file is a row. */
export function downloadOptionRows(options: ModelSizeOption[]): DownloadOptionRow[] {
  if (options.length === 0) return [];

  const withParam = options.map((option) => {
    const match = option.label.match(PARAM_PREFIX_RE);
    return { option, param: match?.[1]?.toUpperCase() ?? null };
  });
  const distinctParams = new Set(withParam.map((row) => row.param).filter(Boolean));
  if (distinctParams.size > 1) {
    return rowsFromGroups(withParam.map((row) => ({ option: row.option, group: row.param })));
  }

  const withBits = options.map((option) => ({
    option,
    group: quantizationBitLabel(quantizationFromLabel(option.label) ?? ''),
  }));
  if (withBits.every((row) => row.group)) {
    return rowsFromGroups(withBits);
  }

  return options.map((option) => ({ heading: null, group: null, option }));
}

function rowsFromGroups(
  items: Array<{ option: ModelSizeOption; group: string | null }>
): DownloadOptionRow[] {
  let last: string | null = null;
  return items.map(({ option, group }) => {
    const heading = group && group !== last ? group : null;
    last = group;
    return { heading, group, option };
  });
}

import type { BrowseModel, ModelSort } from '../types';

function parseParamBillions(raw?: string | null): number | null {
  if (!raw) return null;
  const lower = raw.toLowerCase();

  if (lower.includes('billion')) {
    const token = lower
      .split('billion')[0]
      .trim()
      .split(/\s+/)
      .at(-1)
      ?.replace(/[^0-9.]/g, '');
    const value = token ? Number.parseFloat(token) : Number.NaN;
    return Number.isNaN(value) ? null : value;
  }

  if (lower.includes('million')) {
    const token = lower
      .split('million')[0]
      .trim()
      .split(/\s+/)
      .at(-1)
      ?.replace(/[^0-9.]/g, '');
    const value = token ? Number.parseFloat(token) : Number.NaN;
    return Number.isNaN(value) ? null : value / 1000;
  }

  const compact = lower.match(/([\d.]+)\s*([mb])\b/);
  if (compact) {
    const value = Number.parseFloat(compact[1]);
    if (Number.isNaN(value)) return null;
    return compact[2] === 'm' ? value / 1000 : value;
  }

  const unlabeled = lower.match(/([\d.]+)/);
  if (!unlabeled) return null;
  const value = Number.parseFloat(unlabeled[1]);
  return Number.isNaN(value) ? null : value;
}

function compareOptional(
  left: number | string | null | undefined,
  right: number | string | null | undefined
): number {
  if (left == null && right == null) return 0;
  if (left == null) return 1;
  if (right == null) return -1;
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

export function sortBrowseModels(models: BrowseModel[], sort: ModelSort): BrowseModel[] {
  if (sort === 'relevance') return models;

  const copy = [...models];
  copy.sort((a, b) => {
    switch (sort) {
      case 'name_asc':
        return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
      case 'name_desc':
        return b.name.localeCompare(a.name, undefined, { sensitivity: 'base' });
      case 'size_asc':
        return compareOptional(a.size ?? null, b.size ?? null);
      case 'size_desc':
        return compareOptional(b.size ?? null, a.size ?? null);
      case 'params_asc':
        return compareOptional(
          parseParamBillions(a.details?.parameter_size),
          parseParamBillions(b.details?.parameter_size)
        );
      case 'params_desc':
        return compareOptional(
          parseParamBillions(b.details?.parameter_size),
          parseParamBillions(a.details?.parameter_size)
        );
      case 'updated_asc':
        return compareOptional(a.modified_at ?? null, b.modified_at ?? null);
      case 'updated_desc':
        return compareOptional(b.modified_at ?? null, a.modified_at ?? null);
      default:
        return 0;
    }
  });
  return copy;
}

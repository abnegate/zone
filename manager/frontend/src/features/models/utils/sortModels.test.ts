import { describe, expect, it } from 'bun:test';
import type { BrowseModel } from '../types';
import { sortBrowseModels } from './sortModels';

function model(
  name: string,
  extras: Partial<BrowseModel> = {}
): BrowseModel {
  return { name, ...extras };
}

describe('sortBrowseModels', () => {
  it('leaves relevance order unchanged', () => {
    const models = [model('zeta'), model('alpha')];
    expect(sortBrowseModels(models, 'relevance').map((m) => m.name)).toEqual(['zeta', 'alpha']);
  });

  it('sorts by name case-insensitively', () => {
    const models = [model('zeta'), model('Alpha'), model('beta')];
    expect(sortBrowseModels(models, 'name_asc').map((m) => m.name)).toEqual([
      'Alpha',
      'beta',
      'zeta',
    ]);
    expect(sortBrowseModels(models, 'name_desc').map((m) => m.name)).toEqual([
      'zeta',
      'beta',
      'Alpha',
    ]);
  });

  it('sorts by size and matches backend null ordering', () => {
    const models = [
      model('unknown'),
      model('large', { size: 40 }),
      model('small', { size: 10 }),
    ];
    expect(sortBrowseModels(models, 'size_asc').map((m) => m.name)).toEqual([
      'small',
      'large',
      'unknown',
    ]);
    expect(sortBrowseModels(models, 'size_desc').map((m) => m.name)).toEqual([
      'unknown',
      'large',
      'small',
    ]);
  });

  it('sorts by parameter size', () => {
    const models = [
      model('tiny', { details: { parameter_size: '3B' } }),
      model('huge', { details: { parameter_size: '70B' } }),
      model('mid', { details: { parameter_size: '7 billion' } }),
    ];
    expect(sortBrowseModels(models, 'params_desc').map((m) => m.name)).toEqual([
      'huge',
      'mid',
      'tiny',
    ]);
  });

  it('sorts by updated timestamp', () => {
    const models = [
      model('old', { modified_at: '2024-01-01' }),
      model('new', { modified_at: '2024-03-01' }),
    ];
    expect(sortBrowseModels(models, 'updated_desc').map((m) => m.name)).toEqual(['new', 'old']);
    expect(sortBrowseModels(models, 'updated_asc').map((m) => m.name)).toEqual(['old', 'new']);
  });
});

import { describe, expect, it } from 'bun:test';
import { parse } from '../../validation';
import { SourceResponseSchema, SourceSchema } from './schemas';

const githubConfig = { owner: 'acme', repo: 'zone', branch: 'main' };

describe('SourceSchema', () => {
  it('accepts a fully populated source', () => {
    const source = parse(SourceSchema, {
      id: 'src-1',
      name: 'zone',
      source_type: 'github',
      category: 'file',
      config: githubConfig,
      description: null,
      url: 'https://github.com/acme/zone',
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
    });

    expect(source.category).toBe('file');
    expect(source.url).toBe('https://github.com/acme/zone');
  });

  it('fills category and GitHub URL when the API omits them', () => {
    const source = parse(SourceSchema, {
      id: 'src-1',
      name: 'zone',
      source_type: 'github',
      config: githubConfig,
      description: null,
      url: null,
      is_active: true,
      last_verified_at: null,
      last_error: null,
      created_at: '2024-01-01T00:00:00Z',
      updated_at: '2024-01-01T00:00:00Z',
      index_status: 'pending',
    });

    expect(source.category).toBe('file');
    expect(source.url).toBe('https://github.com/acme/zone');
  });
});

describe('SourceResponseSchema', () => {
  const backendSource = {
    id: 'src-1',
    name: 'zone',
    source_type: 'github',
    config: githubConfig,
    description: null,
    url: null,
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2024-01-01T00:00:00Z',
    updated_at: '2024-01-01T00:00:00Z',
  };

  it('parses a wrapped create response with null url and no category', () => {
    const data = parse(SourceResponseSchema, { source: backendSource });
    expect(data.source.category).toBe('file');
    expect(data.source.url).toBe('https://github.com/acme/zone');
  });

  it('parses an unwrapped backend create response', () => {
    const data = parse(SourceResponseSchema, backendSource);
    expect(data.source.id).toBe('src-1');
    expect(data.source.category).toBe('file');
    expect(data.source.url).toBe('https://github.com/acme/zone');
  });
});

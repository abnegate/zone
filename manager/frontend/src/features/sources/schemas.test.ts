import { describe, expect, it } from 'bun:test';
import { parse } from '../../validation';
import {
  CreateSourceRequestSchema,
  SourceResponseSchema,
  SourceSchema,
  UpdateSourceRequestSchema,
} from './schemas';

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

describe('GitHub installation fields', () => {
  const source = {
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

  it('keeps the installation a newer server sends', () => {
    const parsed = parse(SourceSchema, {
      ...source,
      github_installation_id: 'installation-1',
      has_credentials: false,
      github_installation_status: 'suspended',
      github_account_login: 'acme',
    });

    expect(parsed.github_installation_id).toBe('installation-1');
    expect(parsed.has_credentials).toBe(false);
    expect(parsed.github_installation_status).toBe('suspended');
    expect(parsed.github_account_login).toBe('acme');
  });

  it('accepts a token-backed source with null installation fields', () => {
    const parsed = parse(SourceSchema, {
      ...source,
      github_installation_id: null,
      has_credentials: true,
      github_installation_status: null,
      github_account_login: null,
    });

    expect(parsed.github_installation_id).toBeNull();
    expect(parsed.github_installation_status).toBeNull();
    expect(parsed.github_account_login).toBeNull();
  });

  it('parses a source from an older server without the installation fields', () => {
    const parsed = parse(SourceSchema, source);

    expect(parsed.github_installation_id).toBeUndefined();
    expect(parsed.has_credentials).toBeUndefined();
    expect(parsed.github_installation_status).toBeUndefined();
    expect(parsed.github_account_login).toBeUndefined();
  });

  it('refuses an unknown installation status', () => {
    expect(
      SourceSchema.safeParse({ ...source, github_installation_status: 'deleted' }).success
    ).toBe(false);
  });

  it('accepts an installation on a create request', () => {
    const request = {
      name: 'zone',
      source_type: 'github',
      config: githubConfig,
      github_installation_id: 'installation-1',
    };

    expect(CreateSourceRequestSchema.parse(request)).toEqual(request);
    expect(
      CreateSourceRequestSchema.safeParse({ ...request, github_installation_id: null }).success
    ).toBe(false);
  });

  it('accepts setting or clearing the installation on an update request', () => {
    expect(UpdateSourceRequestSchema.parse({ github_installation_id: 'installation-1' })).toEqual({
      github_installation_id: 'installation-1',
    });
    expect(UpdateSourceRequestSchema.parse({ github_installation_id: null })).toEqual({
      github_installation_id: null,
    });
    expect(UpdateSourceRequestSchema.parse({})).toEqual({});
  });
});

import { describe, expect, it } from 'bun:test';
import type { Project } from '../types';
import { cardDate, formatDate } from './formatters';

const project: Project = {
  id: 'zone',
  name: 'Zone',
  description: null,
  status: 'active',
  github_repo_url: null,
  source_id: null,
  auto: false,
  position: 1,
  last_used_at: null,
  created_at: '',
  updated_at: '',
};

describe('formatDate', () => {
  it('shows nothing for a date it cannot read rather than Invalid Date', () => {
    expect(formatDate('')).toBe('');
    expect(formatDate('not a date')).toBe('');
  });
});

describe('cardDate', () => {
  it('shows nothing for a project the server sent no dates for', () => {
    expect(cardDate(project, 'added')).toBe('');
    expect(cardDate(project, 'custom')).toBe('');
    expect(cardDate(project, 'recent')).toBe('Never used');
  });
});

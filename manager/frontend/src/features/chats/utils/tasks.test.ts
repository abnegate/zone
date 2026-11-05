import { describe, expect, it } from 'bun:test';
import type { ChatTask, ChatTaskRun } from '../types';
import {
  publicationName,
  runProgress,
  runReading,
  taskActive,
  taskStatusLabel,
  taskStatusTint,
} from './tasks';

const RUN: ChatTaskRun = {
  id: 'run-1',
  status: 'running',
  phase: 'acting',
  progress: 42,
  started_at: '2026-11-05T10:00:00Z',
  completed_at: null,
};

const TASK: ChatTask = {
  id: 'task-1',
  title: 'Fix the build',
  title_pending: false,
  status: 'in_progress',
  is_agentic: true,
  href: '/tasks?id=task-1',
  created_at: '2026-11-05T10:00:00Z',
  updated_at: '2026-11-05T10:00:00Z',
  run: RUN,
  pull_request: null,
};

describe('chat task readings', () => {
  it('names and tints every task status', () => {
    expect(taskStatusLabel('in_progress')).toBe('In progress');
    expect(taskStatusLabel('review')).toBe('In review');
    expect(taskStatusTint('complete')).toBe('success');
    expect(taskStatusTint('blocked')).toBe('destructive');
  });

  it('reads a running run with what it is doing', () => {
    expect(runReading(RUN)).toBe('Running, using tools');
    expect(runReading({ ...RUN, phase: null })).toBe('Running');
    expect(runReading({ ...RUN, status: 'waiting', phase: 'waiting' })).toBe('Waiting');
  });

  it('reads a finished run by how it ended, whatever phase it stopped in', () => {
    expect(runReading({ ...RUN, status: 'failed', phase: 'acting' })).toBe('Run failed');
    expect(runReading({ ...RUN, status: 'completed', phase: 'complete' })).toBe('Run completed');
    expect(runReading(null)).toBeNull();
  });

  it('shows progress only while the run is going, clamped to a percentage', () => {
    expect(runProgress(RUN)).toBe(42);
    expect(runProgress({ ...RUN, progress: 140 })).toBe(100);
    expect(runProgress({ ...RUN, progress: null })).toBeNull();
    expect(runProgress({ ...RUN, status: 'completed', progress: 100 })).toBeNull();
    expect(runProgress(null)).toBeNull();
  });

  it('keeps watching a task while its run is going or it waits to be picked up', () => {
    expect(taskActive(TASK)).toBe(true);
    expect(taskActive({ ...TASK, run: { ...RUN, status: 'waiting' } })).toBe(true);
    expect(taskActive({ ...TASK, run: { ...RUN, status: 'failed' } })).toBe(false);
    expect(taskActive({ ...TASK, run: null, status: 'queued' })).toBe(true);
    expect(taskActive({ ...TASK, run: null, status: 'created' })).toBe(false);
  });

  it('names a GitHub pull request by its repository and number', () => {
    expect(publicationName('https://github.com/acme/widgets/pull/12')).toBe('widgets #12');
    expect(publicationName('https://git.example.com/acme/widgets/merge/3')).toBe('Pull request');
  });
});

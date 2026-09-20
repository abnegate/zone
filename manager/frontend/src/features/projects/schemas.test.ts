import { describe, expect, it } from 'bun:test';
import {
  AutoProjectRequestSchema,
  AutoProjectResponseSchema,
  ProjectAutomationSchema,
  ProjectSchema,
  UpdateProjectRequestSchema,
} from './schemas';

const project = {
  id: 'proj-1',
  name: 'Alpha',
  description: null,
  status: 'active',
  github_repo_url: null,
  source_id: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

describe('project automation schemas', () => {
  it('treats a project from an older server as not automated', () => {
    const parsed = ProjectSchema.parse(project);
    expect(parsed.auto).toBe(false);
    expect(parsed.auto_paused_reason).toBeUndefined();
  });

  it('keeps the automation fields a newer server sends', () => {
    const parsed = ProjectSchema.parse({
      ...project,
      auto: true,
      auto_paused_reason: 'lost write access',
      auto_completed_at: null,
    });
    expect(parsed.auto).toBe(true);
    expect(parsed.auto_paused_reason).toBe('lost write access');
    expect(parsed.auto_completed_at).toBeNull();
  });

  it('requires a brief to start an auto project', () => {
    expect(AutoProjectRequestSchema.safeParse({ brief: '' }).success).toBe(false);
    expect(AutoProjectRequestSchema.safeParse({ brief: 'x'.repeat(8001) }).success).toBe(false);
    expect(
      AutoProjectRequestSchema.safeParse({ brief: 'A recipe app', model_name: 'qwen' }).success
    ).toBe(true);
    expect(AutoProjectResponseSchema.parse({ chat_id: 'c' }).chat_id).toBe('c');
  });

  it('accepts the auto flag on an update', () => {
    expect(UpdateProjectRequestSchema.parse({ auto: true })).toEqual({ auto: true });
    expect(UpdateProjectRequestSchema.safeParse({ auto: 'yes' }).success).toBe(false);
  });

  it('parses an automation report and refuses an unknown stage', () => {
    const report = {
      project_id: 'proj-1',
      auto: true,
      actor_id: null,
      paused_reason: null,
      completed_at: null,
      parallelism: 3,
      planner_chat_id: null,
      updates_chat_id: null,
      counts: { total: 1, agentic: 1, complete: 0, in_flight: 1, paused: 0 },
      tasks: [
        {
          task_id: 't',
          title: 'Scaffold',
          status: 'in_progress',
          is_agentic: true,
          kind: 'scaffold',
          stage: 'awaiting_reviews',
          reason: null,
          runs: 1,
          review_rounds: 1,
          reviewers: 'other-model',
          pr_url: 'https://github.com/acme/app/pull/1',
          head: 'abc',
          checks: 'success',
          merge_sha: null,
          auto_created: false,
        },
      ],
    };
    expect(ProjectAutomationSchema.parse(report).tasks[0].stage).toBe('awaiting_reviews');
    const bad = { ...report, tasks: [{ ...report.tasks[0], stage: 'dancing' }] };
    expect(ProjectAutomationSchema.safeParse(bad).success).toBe(false);
  });
});

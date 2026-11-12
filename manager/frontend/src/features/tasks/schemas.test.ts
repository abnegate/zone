import { describe, expect, it } from 'bun:test';
import created from '../../../../../runner/zone_server/tests/fixtures/task-created.json';
import populated from '../../../../../runner/zone_server/tests/fixtures/task-populated.json';
import {
  RunStatusSchema,
  TaskProgressMessageSchema,
  TaskResponseSchema,
  TaskRunSchema,
  TaskStatusSchema,
  TasksResponseSchema,
  UpdateTasksRequestSchema,
} from './schemas';

describe('server task response contract', () => {
  for (const [name, response] of Object.entries({ created, populated })) {
    it(`accepts the ${name} response serialized by the server`, () => {
      expect(TaskResponseSchema.safeParse(response).success).toBe(true);
      expect(TasksResponseSchema.safeParse({ tasks: [response.task] }).success).toBe(true);
    });
  }
});

describe('task status', () => {
  it.each(['paused', 'cancelled'])('accepts %s', (status) => {
    expect(TaskStatusSchema.parse(status)).toBe(status);
  });

  it('accepts a task the server sends as paused or cancelled', () => {
    for (const status of ['paused', 'cancelled']) {
      const task = { ...populated.task, status };
      expect(TaskResponseSchema.parse({ task }).task.status).toBe(status);
    }
  });

  it('rejects a status the server never writes', () => {
    expect(TaskStatusSchema.safeParse('archived').success).toBe(false);
  });
});

describe('an edit of many tasks', () => {
  const ids = ['task-1', 'task-2'];

  it('needs at least one task to edit', () => {
    expect(UpdateTasksRequestSchema.safeParse({ ids: [], status: 'paused' }).success).toBe(false);
    expect(UpdateTasksRequestSchema.safeParse({ status: 'paused' }).success).toBe(false);
  });

  it.each([
    { status: 'paused' },
    { priority: 5 },
    { project_ids: [] },
    { project_ids: ['project-1'] },
    { is_agentic: false },
    { require_plan_approval: true },
    {
      status: 'blocked',
      priority: 1,
      project_ids: ['project-1'],
      is_agentic: true,
      require_plan_approval: false,
    },
  ])('carries only the changes it was given: %p', (changes) => {
    expect(UpdateTasksRequestSchema.parse({ ids, ...changes })).toEqual({ ids, ...changes });
  });

  it('rejects a status the server never writes', () => {
    expect(UpdateTasksRequestSchema.safeParse({ ids, status: 'archived' }).success).toBe(false);
  });

  it('requires only the ids, so every change may be left out', () => {
    const required = Object.entries(UpdateTasksRequestSchema.shape)
      .filter(([, field]) => !field.isOptional())
      .map(([name]) => name);

    expect(required).toEqual(['ids']);
  });
});

const run = {
  id: 'run-1',
  task_id: 'task-1',
  status: 'waiting',
  current_phase: 'acting',
  progress_percent: 40,
  error_message: null,
};

const question = {
  header: 'Scope',
  question: 'How far should this go?',
  choices: [
    {
      label: 'Backfill',
      description: 'Rewrite every existing row.',
      recommended: true,
      free_text: false,
    },
  ],
  multi_select: false,
  required: true,
};

describe('a run that parked on a question', () => {
  it('accepts the waiting status the worker writes when it asks', () => {
    expect(RunStatusSchema.safeParse('waiting').success).toBe(true);
  });

  it('accepts the waiting status streamed over the run socket', () => {
    const message = {
      type: 'status_update',
      status: 'waiting',
      current_phase: 'acting',
      progress_percent: 40,
    };

    expect(TaskProgressMessageSchema.parse(message)).toMatchObject({ status: 'waiting' });
  });

  it('carries the question the run is parked on', () => {
    const pending_question = { tool_call_id: 'call-1', questions: [question] };

    expect(TaskRunSchema.parse({ ...run, pending_question })).toMatchObject({ pending_question });
  });

  it('accepts a run that carries no question, sent or stored before the field existed', () => {
    expect(TaskRunSchema.parse(run)).not.toHaveProperty('pending_question');
    expect(TaskRunSchema.parse({ ...run, pending_question: null }).pending_question).toBeNull();
  });

  it('costs an unreadable question the card, never the run it sits on', () => {
    const parsed = TaskRunSchema.parse({ ...run, pending_question: { questions: 'not a list' } });

    expect(parsed.pending_question).toBeUndefined();
    expect(parsed).toMatchObject({ id: 'run-1', status: 'waiting', progress_percent: 40 });
  });
});

const waiting_on = {
  kind: 'check',
  id: 'a1b2c3d4e5f6',
  reference: 'main',
  deadline: '2026-09-13T17:00:00Z',
};

describe('a run that parked on a wait', () => {
  it('carries the wait the run is parked on', () => {
    expect(TaskRunSchema.parse({ ...run, waiting_on })).toMatchObject({ waiting_on });
  });

  it('carries a wait with nothing to reference, as a job or another run has', () => {
    const { reference: _, ...job } = { ...waiting_on, kind: 'job' };

    expect(TaskRunSchema.parse({ ...run, waiting_on: job }).waiting_on).toEqual(job);
  });

  it('accepts a run that carries no wait, sent or stored before the field existed', () => {
    expect(TaskRunSchema.parse(run)).not.toHaveProperty('waiting_on');
    expect(TaskRunSchema.parse({ ...run, waiting_on: null }).waiting_on).toBeNull();
  });

  it('costs an unreadable wait its subject, never the run it sits on', () => {
    const parsed = TaskRunSchema.parse({ ...run, waiting_on: { kind: 'job', deadline: 7 } });

    expect(parsed.waiting_on).toBeUndefined();
    expect(parsed).toMatchObject({ id: 'run-1', status: 'waiting', progress_percent: 40 });
  });
});

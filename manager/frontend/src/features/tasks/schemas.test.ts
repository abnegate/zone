import { describe, expect, it } from 'bun:test';
import created from '../../../../../runner/zone_server/tests/fixtures/task-created.json';
import populated from '../../../../../runner/zone_server/tests/fixtures/task-populated.json';
import { TaskResponseSchema, TasksResponseSchema } from './schemas';

describe('server task response contract', () => {
  for (const [name, response] of Object.entries({ created, populated })) {
    it(`accepts the ${name} response serialized by the server`, () => {
      expect(TaskResponseSchema.safeParse(response).success).toBe(true);
      expect(TasksResponseSchema.safeParse({ tasks: [response.task] }).success).toBe(true);
    });
  }
});

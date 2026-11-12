import type { TaskChanges } from './types';

export class Edits {
  static readonly NONE: Edits = new Edits({});

  private constructor(private readonly changes: Readonly<TaskChanges>) {}

  with<K extends keyof TaskChanges>(key: K, value: TaskChanges[K] | undefined): Edits {
    const changes: TaskChanges = { ...this.changes };
    if (value === undefined) delete changes[key];
    else changes[key] = Array.isArray(value) ? ([...value] as TaskChanges[K]) : value;
    return new Edits(changes);
  }

  get touched(): boolean {
    return Object.keys(this.changes).length > 0;
  }

  request(): TaskChanges {
    const request: TaskChanges = { ...this.changes };
    if (request.project_ids) request.project_ids = [...request.project_ids];
    return request;
  }
}

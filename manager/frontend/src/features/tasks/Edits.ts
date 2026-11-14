import type { TaskChanges, TaskSettings } from './types';

export const ProjectEdit = {
  ADD: 'add',
  REMOVE: 'remove',
  UNCHANGED: 'unchanged',
} as const;

export type ProjectEdit = (typeof ProjectEdit)[keyof typeof ProjectEdit];

type ProjectChange = Exclude<ProjectEdit, typeof ProjectEdit.UNCHANGED>;

export class Edits {
  static readonly NONE: Edits = new Edits({}, new Map());

  private constructor(
    private readonly settings: Readonly<TaskSettings>,
    private readonly projects: ReadonlyMap<string, ProjectChange>
  ) {}

  with<K extends keyof TaskSettings>(key: K, value: TaskSettings[K] | undefined): Edits {
    const settings: TaskSettings = { ...this.settings };
    if (value === undefined) delete settings[key];
    else settings[key] = value;
    return new Edits(settings, this.projects);
  }

  project(id: string, edit: ProjectEdit): Edits {
    const projects = new Map(this.projects);
    if (edit === ProjectEdit.UNCHANGED) projects.delete(id);
    else projects.set(id, edit);
    return new Edits(this.settings, projects);
  }

  projectEdit(id: string): ProjectEdit {
    return this.projects.get(id) ?? ProjectEdit.UNCHANGED;
  }

  get touched(): boolean {
    return Object.keys(this.settings).length > 0 || this.projects.size > 0;
  }

  request(): TaskChanges {
    const request: TaskChanges = { ...this.settings };
    const added = this.projectsTo(ProjectEdit.ADD);
    const removed = this.projectsTo(ProjectEdit.REMOVE);
    if (added.length > 0) request.add_project_ids = added;
    if (removed.length > 0) request.remove_project_ids = removed;
    return request;
  }

  private projectsTo(change: ProjectChange): string[] {
    return [...this.projects].filter(([, edit]) => edit === change).map(([id]) => id);
  }
}

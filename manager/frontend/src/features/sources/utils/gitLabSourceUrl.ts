import { gitLabProjectUrl } from './gitLabProjectUrl';

const INSTANCE_FIELDS = ['host', 'base_url'];
const PROJECT_FIELDS = ['project_id', 'project'];

function named(config: Record<string, unknown>, fields: string[]): unknown[] {
  return fields.map((field) => config[field]).filter((value) => value !== undefined);
}

export function gitLabSourceUrl(config: Record<string, unknown>): string | undefined {
  const instances = named(config, INSTANCE_FIELDS);
  const projects = named(config, PROJECT_FIELDS);
  if (instances.length > 1 || projects.length > 1) return undefined;
  const [instance] = instances;
  const [project] = projects;
  if (instance !== undefined && instance !== null && typeof instance !== 'string') {
    return undefined;
  }
  if (typeof project !== 'string') return undefined;
  return gitLabProjectUrl(instance ?? undefined, project);
}

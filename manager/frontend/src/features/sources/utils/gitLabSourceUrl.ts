import { gitLabProjectUrl } from './gitLabProjectUrl';

export function gitLabSourceUrl(config: Record<string, unknown>): string | undefined {
  const named = [config.host, config.base_url].filter((instance) => instance !== undefined);
  if (named.length > 1) return undefined;
  const [instance] = named;
  if (instance !== undefined && instance !== null && typeof instance !== 'string') {
    return undefined;
  }
  if (typeof config.project_id !== 'string') return undefined;
  return gitLabProjectUrl(instance ?? undefined, config.project_id);
}

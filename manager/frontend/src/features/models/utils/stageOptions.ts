import type { InstalledModel } from '../types';

export function mergeStageOptions(
  suggested: string[],
  installed: InstalledModel[],
  current: string,
  kind: 'chat' | 'embedding'
): string[] {
  const extra = installed
    .filter((model) =>
      kind === 'embedding'
        ? model.completion === false || model.capabilities?.includes('embeddings')
        : model.completion !== false && !model.capabilities?.includes('embeddings')
    )
    .map((model) => model.name);
  return [...new Set([...suggested, ...extra, current].filter(Boolean))];
}

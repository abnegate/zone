export function sourcesQueryKey(workspaceId: string | undefined): readonly unknown[] {
  return ['sources', workspaceId];
}

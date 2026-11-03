import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { WorkspaceChatProviders } from '../types';

const LOAD_FAILED = 'Failed to load AI providers';

/**
 * The providers a new chat in this workspace may be created on, and the
 * workspace default it follows when none is chosen.
 */
export function useWorkspaceChatProviders(workspaceId: string | null) {
  const [providers, setProviders] = useState<WorkspaceChatProviders | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const load = useCallback(async (id: string | null): Promise<void> => {
    const current = ++request.current;
    setError(null);
    if (!id) {
      setProviders(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const loaded = await chatsApi.getWorkspaceChatProviders(id);
      if (current === request.current) setProviders(loaded);
    } catch (error) {
      if (current !== request.current) return;
      setError(error instanceof Error ? error.message : LOAD_FAILED);
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    setProviders(null);
    void load(workspaceId);
  }, [workspaceId, load]);

  const refresh = useCallback((): Promise<void> => load(workspaceId), [workspaceId, load]);

  return { providers, loading, error, refresh };
}

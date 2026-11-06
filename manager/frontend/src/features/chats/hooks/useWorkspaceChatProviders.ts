import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { WorkspaceChatProviders } from '../types';

const LOAD_FAILED = 'Failed to load AI providers';

interface Loaded {
  workspaceId: string;
  providers: WorkspaceChatProviders;
}

/**
 * The providers a new chat in this workspace may be created on, and the
 * workspace default it follows when none is chosen.
 */
export function useWorkspaceChatProviders(workspaceId: string | null) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);

  const load = useCallback(async (id: string | null): Promise<void> => {
    const current = ++request.current;
    setError(null);
    if (!id) {
      setLoaded(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const providers = await chatsApi.getWorkspaceChatProviders(id);
      if (current === request.current) setLoaded({ workspaceId: id, providers });
    } catch (error) {
      if (current !== request.current) return;
      setError(error instanceof Error ? error.message : LOAD_FAILED);
    } finally {
      if (current === request.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    setLoaded(null);
    void load(workspaceId);
  }, [workspaceId, load]);

  const providers = loaded?.workspaceId === workspaceId ? loaded.providers : null;

  const refresh = useCallback((): Promise<void> => load(workspaceId), [workspaceId, load]);

  return { providers, loading, error, refresh };
}

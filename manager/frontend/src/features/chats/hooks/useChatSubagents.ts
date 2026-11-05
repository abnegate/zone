import { useCallback, useEffect, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import { ResyncFrameSchema, SubagentFrameSchema } from '../subagentSchemas';
import type { ChatSubagent } from '../subagentTypes';
import { subscribeFrames } from '../utils/frames';
import { mergeSubagent, mergeSubagents } from '../utils/subagents';
import { RECORDING_SETTLE_DELAY } from './useChatPullRequests';

export interface ChatSubagents {
  subagents: ChatSubagent[];
  error: string | null;
  refresh: () => Promise<void>;
}

interface Reading {
  chatId: string | null;
  subagents: ChatSubagent[];
  error: string | null;
}

const NOTHING_READ: Reading = { chatId: null, subagents: [], error: null };

const LOAD_FAILED = 'Failed to load agents';

function held(reading: Reading, chatId: string): ChatSubagent[] {
  return reading.chatId === chatId ? reading.subagents : [];
}

/**
 * The chat's subagents: read once per chat and kept current by the socket's
 * subagent frames. Every read is merged rather than swapped in, so a frame
 * that lands while a read is in flight is never undone by the older answer.
 */
export function useChatSubagents(chatId: string | null, streaming: boolean): ChatSubagents {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const wasStreaming = useRef(streaming);
  const current = reading.chatId === chatId ? reading : NOTHING_READ;

  const refresh = useCallback(async () => {
    const attempt = ++read.current;
    if (!chatId) {
      setReading(NOTHING_READ);
      return;
    }
    try {
      const loaded = await chatsApi.getChatSubagents(chatId);
      if (attempt !== read.current) return;
      setReading((last) => ({
        chatId,
        subagents: mergeSubagents(held(last, chatId), loaded),
        error: null,
      }));
    } catch (failure) {
      if (attempt !== read.current) return;
      const error = failure instanceof Error ? failure.message : LOAD_FAILED;
      setReading((last) => ({ chatId, subagents: held(last, chatId), error }));
    }
  }, [chatId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (streaming) return;
    void refresh();
    const settled = setTimeout(() => void refresh(), RECORDING_SETTLE_DELAY);
    return () => clearTimeout(settled);
  }, [streaming, refresh]);

  useEffect(() => {
    if (!chatId) return;
    return subscribeFrames(chatId, (frame) => {
      const update = SubagentFrameSchema.safeParse(frame);
      if (update.success) {
        if (update.data.chat_id !== chatId) return;
        const incoming = update.data.subagent;
        setReading((last) => {
          if (last.chatId !== chatId) return { chatId, subagents: [incoming], error: null };
          const subagents = mergeSubagent(last.subagents, incoming);
          return subagents === last.subagents ? last : { ...last, subagents };
        });
        return;
      }
      if (ResyncFrameSchema.safeParse(frame).success) void refresh();
    });
  }, [chatId, refresh]);

  return { subagents: current.subagents, error: current.error, refresh };
}

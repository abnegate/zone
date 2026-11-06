import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import {
  INIT_FRAME,
  MESSAGE_START_FRAME,
  ResumedStartFrameSchema,
  SUBAGENT_FRAME,
  SUBAGENTS_LOAD_FAILED,
  SUBAGENTS_REMOVED_FRAME,
  SubagentFrameSchema,
  SubagentsRemovedFrameSchema,
} from '../subagentSchemas';
import type { ChatSubagent, SubagentStep } from '../subagentTypes';
import { frameType, subscribeFrames } from '../utils/frames';
import {
  absentSubagents,
  groupByMessage,
  mergeSteps,
  mergeSubagent,
  mergeSubagents,
  withoutSubagents,
} from '../utils/subagents';

/** How long after a turn ends its last subagent writes get to land before one more read. */
export const SETTLE_DELAY = 3_000;

export type SubagentStepLists = Readonly<Record<string, SubagentStep[] | undefined>>;

export interface ChatSubagents {
  subagents: ChatSubagent[];
  byMessage: ReadonlyMap<string, ChatSubagent[]>;
  /** The steps of each agent whose steps were asked for, once they have been read. */
  steps: SubagentStepLists;
  error: string | null;
  refresh: () => Promise<void>;
  /** Reads an agent's steps and keeps them current from then on; rejects when the read fails. */
  loadSteps: (id: string) => Promise<void>;
}

interface StepCache {
  steps: SubagentStep[];
  loaded: boolean;
}

type StepCaches = Readonly<Record<string, StepCache>>;

interface Reading {
  chatId: string | null;
  subagents: ChatSubagent[];
  steps: StepCaches;
  error: string | null;
}

const NO_STEPS: StepCaches = {};
const NOTHING_READ: Reading = { chatId: null, subagents: [], steps: NO_STEPS, error: null };

function forChat(reading: Reading, chatId: string): Reading {
  return reading.chatId === chatId ? reading : { ...NOTHING_READ, chatId };
}

function loadedSteps(caches: StepCaches): SubagentStepLists {
  const lists: Record<string, SubagentStep[]> = {};
  for (const [id, cache] of Object.entries(caches)) {
    if (cache.loaded) lists[id] = cache.steps;
  }
  return lists;
}

function withoutSteps(caches: StepCaches, ids: ReadonlySet<string>): StepCaches {
  if (!Object.keys(caches).some((id) => ids.has(id))) return caches;
  return Object.fromEntries(Object.entries(caches).filter(([id]) => !ids.has(id)));
}

/**
 * The chat's subagents: read once per chat and kept current by the socket's
 * subagent frames. A read decides which agents exist, except those a frame
 * announced while it was in flight; the rest are merged rather than swapped
 * in, so a frame that lands mid-read is never undone by the older answer.
 * Steps are read only for the agents someone asked to see, then kept current
 * from each frame's changed steps.
 */
export function useChatSubagents(chatId: string | null, streaming: boolean): ChatSubagents {
  const [reading, setReading] = useState<Reading>(NOTHING_READ);
  const read = useRef(0);
  const answered = useRef<string | null>(null);
  const requested = useRef(new Set<string>());
  const removed = useRef(new Set<string>());
  const announced = useRef(new Set<string>());
  const stepReads = useRef(new Map<string, number>());
  const wasStreaming = useRef(streaming);
  const current = reading.chatId === chatId ? reading : NOTHING_READ;

  const forget = useCallback((ids: ReadonlySet<string>) => {
    for (const id of ids) {
      removed.current.add(id);
      requested.current.delete(id);
      stepReads.current.delete(id);
    }
  }, []);

  const refresh = useCallback(async () => {
    const attempt = ++read.current;
    announced.current = new Set();
    if (!chatId) {
      setReading(NOTHING_READ);
      return;
    }
    try {
      const answer = await chatsApi.getChatSubagents(chatId);
      if (attempt !== read.current) return;
      const loaded = withoutSubagents(answer, removed.current);
      const kept = new Set([...announced.current, ...loaded.map((subagent) => subagent.id)]);
      answered.current = chatId;
      setReading((last) => {
        const base = forChat(last, chatId);
        const gone = absentSubagents(base.subagents, kept);
        forget(gone);
        return {
          ...base,
          subagents: mergeSubagents(withoutSubagents(base.subagents, gone), loaded),
          steps: withoutSteps(base.steps, gone),
          error: null,
        };
      });
    } catch (failure) {
      if (attempt !== read.current) return;
      answered.current = chatId;
      const error = failure instanceof Error ? failure.message : SUBAGENTS_LOAD_FAILED;
      setReading((last) => ({ ...forChat(last, chatId), error }));
    }
  }, [chatId, forget]);

  const loadSteps = useCallback(
    async (id: string) => {
      if (!chatId) return;
      const reads = stepReads.current;
      const attempt = (reads.get(id) ?? 0) + 1;
      reads.set(id, attempt);
      requested.current.add(id);
      setReading((last) => {
        const base = forChat(last, chatId);
        if (base.steps[id]) return base;
        return { ...base, steps: { ...base.steps, [id]: { steps: [], loaded: false } } };
      });
      const stale = () => reads !== stepReads.current || reads.get(id) !== attempt;
      let loaded: SubagentStep[];
      try {
        loaded = await chatsApi.getChatSubagentSteps(chatId, id);
      } catch (failure) {
        if (stale()) return;
        throw failure;
      }
      if (stale()) return;
      setReading((last) => {
        const cache = last.chatId === chatId ? last.steps[id] : undefined;
        if (!cache) return last;
        const steps = mergeSteps(loaded, cache.steps);
        return { ...last, steps: { ...last.steps, [id]: { steps, loaded: true } } };
      });
    },
    [chatId]
  );

  const resync = useCallback(() => {
    if (answered.current !== chatId) return;
    void refresh();
    for (const id of requested.current) void loadSteps(id).catch(() => undefined);
  }, [chatId, refresh, loadSteps]);

  useEffect(() => {
    answered.current = null;
    requested.current = new Set();
    removed.current = new Set();
    stepReads.current = new Map();
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (streaming) return;
    void refresh();
    const settled = setTimeout(() => void refresh(), SETTLE_DELAY);
    return () => clearTimeout(settled);
  }, [streaming, refresh]);

  useEffect(() => {
    if (!chatId) return;

    const receive = (frame: unknown) => {
      const update = SubagentFrameSchema.safeParse(frame);
      if (!update.success || update.data.chat_id !== chatId) return;
      const { subagent, steps } = update.data;
      announced.current.add(subagent.id);
      setReading((last) => {
        const base = forChat(last, chatId);
        const subagents = mergeSubagent(base.subagents, subagent);
        const cache = base.steps[subagent.id];
        const merged = cache ? mergeSteps(cache.steps, steps) : undefined;
        if (!cache || !merged || merged === cache.steps) {
          return subagents === base.subagents ? base : { ...base, subagents };
        }
        return {
          ...base,
          subagents,
          steps: { ...base.steps, [subagent.id]: { ...cache, steps: merged } },
        };
      });
    };

    const remove = (frame: unknown) => {
      const removal = SubagentsRemovedFrameSchema.safeParse(frame);
      if (!removal.success || removal.data.chat_id !== chatId) return;
      const ids = new Set(removal.data.ids);
      forget(ids);
      setReading((last) => {
        if (last.chatId !== chatId) return last;
        const subagents = withoutSubagents(last.subagents, ids);
        const steps = withoutSteps(last.steps, ids);
        if (subagents === last.subagents && steps === last.steps) return last;
        return { ...last, subagents, steps };
      });
    };

    return subscribeFrames(chatId, (frame) => {
      switch (frameType(frame)) {
        case SUBAGENT_FRAME:
          receive(frame);
          return;
        case SUBAGENTS_REMOVED_FRAME:
          remove(frame);
          return;
        case INIT_FRAME:
          resync();
          return;
        case MESSAGE_START_FRAME:
          if (ResumedStartFrameSchema.safeParse(frame).success) resync();
          return;
      }
    });
  }, [chatId, resync, forget]);

  const byMessage = useMemo(() => groupByMessage(current.subagents), [current.subagents]);
  const steps = useMemo(() => loadedSteps(current.steps), [current.steps]);

  return {
    subagents: current.subagents,
    byMessage,
    steps,
    error: current.error,
    refresh,
    loadSteps,
  };
}

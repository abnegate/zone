import type { ChatSubagent } from '../subagentTypes';
import type {
  ChatProviders,
  ChatPullRequest,
  ChatSource,
  ChatTask,
  ChatWithMessages,
} from '../types';
import { ChatCache } from './cache';
import type { GenerationClock } from './generation';

/** Everything the console reads about a chat, kept per chat so reopening it draws at once. */
export const cached = {
  histories: new ChatCache<ChatWithMessages>(),
  pullRequests: new ChatCache<ChatPullRequest[]>(),
  tasks: new ChatCache<ChatTask[]>(),
  subagents: new ChatCache<ChatSubagent[]>(),
  sources: new ChatCache<ChatSource[]>(),
  providers: new ChatCache<ChatProviders>(),
  generations: new ChatCache<GenerationClock>(),
} as const;

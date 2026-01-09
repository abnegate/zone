// Types
export type {
  MessageRole,
  Message,
  Chat,
  ChatWithMessages,
  CreateChatRequest,
  SendMessageRequest,
  ChatSearchResult,
  ChatSearchOptions,
  ChatSearchResponse,
} from './types';

// Hooks
export { useChats, useChat, useChatSearch } from './hooks';
export type { UseChatsOptions } from './hooks';

// Components
export * from './components';

// Pages
export { ChatsPage } from './pages';

// Schemas (for external validation needs)
export {
  MessageRoleSchema,
  MessageSchema,
  ChatSchema,
  ChatWithMessagesSchema,
  CreateChatRequestSchema,
  SendMessageRequestSchema,
  ChatsResponseSchema,
  ChatResponseSchema,
  MessagesResponseSchema,
  MessageResponseSchema,
  ChatSearchResultSchema,
  ChatSearchResponseSchema,
} from './schemas';

import {
  Button,
  Checkbox,
  EmptyState,
  Input,
  Modal,
  Select,
  Tabs,
  TabsList,
  TabsTrigger,
} from '@zone/ui';
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { isProtectedArtifactUrl } from '../../../api/protectedImages';
import { useAuth } from '../../../features/auth';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import SubNav from '../../../shared/components/SubNav/SubNav';
import TransferChip from '../../../shared/components/TransferChip/TransferChip';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { useTransfers } from '../../../shared/hooks/useTransfers';
import { useModels } from '../../models';
import { useProjects } from '../../projects/hooks';
import type { AiProvider } from '../../settings/workspace/types';
import { useSources } from '../../sources/hooks/useSources';
import {
  ActionReceipts,
  AgentPermission,
  AgentPermissionSummary,
  AuthenticatedAudio,
  AuthenticatedImage,
  AuthenticatedVideo,
  AutomationBadge,
  ChatMenu,
  ChatPanel,
  ChatSources,
  Generation,
  HandoverNotice,
  HeaderPopover,
  HeaderSelect,
  MemoryBadge,
  MessageContent,
  ProviderError,
  ProviderPicker,
  SourcesButton,
} from '../components';
import { Activity } from '../components/Activity';
import { ContextUsage } from '../components/ContextUsage';
import {
  useChat,
  useChatProviders,
  useChatPullRequests,
  useChatSearch,
  useChatSources,
  useChats,
  useChatTasks,
  useWorkspaceChatProviders,
} from '../hooks';
import {
  type Chat,
  type ChatProviderOffer,
  type ChatSearchResult,
  REASONING_EFFORT_LABELS,
  REASONING_EFFORT_OPTIONS,
  type ReasoningEffort,
} from '../types';
import {
  type Attachment,
  AUTO_EFFORT,
  AUTO_MODEL,
  arrangeChats,
  asksModelName,
  attachmentId,
  attachmentMetadata,
  audioAttachments,
  automated,
  buildMessageWithAttachments,
  CHAT_GROUP_OPTIONS,
  CHAT_SORT_OPTIONS,
  chatModelName,
  chatProviderName,
  chatShowsAgent,
  chatShowsCharacter,
  chatShowsReasoning,
  chatSources,
  contextTokenOptions,
  DEFAULT_PROVIDER,
  defaultContextTokens,
  effectiveOffer,
  effortsFor,
  findInstalledModel,
  formatBytes,
  formatDate,
  galleryItems,
  imageAttachments,
  isAgentOffer,
  isChatGroupBy,
  isChatSort,
  isSendable,
  isStartingImage,
  keepEffort,
  MODEL_NAME_PLACEHOLDER,
  MODEL_PLACEHOLDER,
  modelLabel,
  modelSelectOptions,
  observationKey,
  offersAgent,
  offersAgentOn,
  offersContextOn,
  parseCharacterFile,
  parseCharacterText,
  providerChoices,
  readAttachment,
  readChatGroupBy,
  readChatSort,
  reasoningOptions,
  requestedModel,
  type SourceRequest,
  selectedContextTokens,
  shortModelName,
  sourceAttachment,
  splitAtHandovers,
  toPlainText,
  videoAttachments,
  writeChatGroupBy,
  writeChatSort,
} from '../utils';
import './ChatsPage.css';

const UNTITLED_CHAT_TITLE = 'Untitled chat';
const NO_PROJECT = 'none';
const NEW_CHAT_MODEL_REQUIRED = 'Choose a model for this provider.';
const PROVIDER_EFFORT_RESET_FAILED =
  'AI provider changed, but its reasoning effort could not be reset';
const MODEL_EFFORT_RESET_FAILED = 'Model changed, but its reasoning effort could not be reset';

const failure = (prefix: string, error: unknown): string =>
  error instanceof Error ? `${prefix}: ${error.message}` : prefix;

export default function ChatsPage() {
  const { isAuthenticated, user } = useAuth();
  const { currentWorkspace } = useWorkspace();
  const { models, error: modelsError } = useModels(currentWorkspace?.id);
  const {
    providers: workspaceProviders,
    error: workspaceProvidersError,
    refresh: refreshWorkspaceProviders,
  } = useWorkspaceChatProviders(currentWorkspace?.id ?? null);

  const [searchParams, setSearchParams] = useSearchParams();
  const [showArchived, setShowArchived] = useState(false);
  const [selectedChatId, setSelectedChatId] = useState<string | null>(() => searchParams.get('id'));
  const linkedMessageId = searchParams.get('message');
  const [showNewChatModal, setShowNewChatModal] = useState(false);
  const [newChatProvider, setNewChatProvider] = useState(DEFAULT_PROVIDER);
  const [newChatModel, setNewChatModel] = useState(AUTO_MODEL);
  const [newChatAgent, setNewChatAgent] = useState(false);
  const [newChatAutoApprove, setNewChatAutoApprove] = useState(false);
  const [newChatReasoning, setNewChatReasoning] = useState<ReasoningEffort>(AUTO_EFFORT);
  const [newChatProject, setNewChatProject] = useState(NO_PROJECT);
  const [newChatOffline, setNewChatOffline] = useState(false);
  const [newChatContext, setNewChatContext] = useState(String(defaultContextTokens()));
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);
  const [renameId, setRenameId] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [renameError, setRenameError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const renamePending = useRef(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [showSearchResults, setShowSearchResults] = useState(false);
  const [groupBy, setGroupBy] = useState(readChatGroupBy);
  const [sort, setSort] = useState(readChatSort);
  const [messageInput, setMessageInput] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [showCharacterModal, setShowCharacterModal] = useState(false);
  const [characterDraft, setCharacterDraft] = useState('');
  const [characterError, setCharacterError] = useState<string | null>(null);
  const [characterSaving, setCharacterSaving] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const characterFileRef = useRef<HTMLInputElement>(null);

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  };

  const attaching = useTransfers<Attachment>({
    run: (file, { signal, onProgress }) => readAttachment(file, { signal, onProgress }),
    identify: attachmentId,
    onDone: (attachment) => {
      setAttachments((prev) =>
        prev.some((item) => item.id === attachment.id) ? prev : [...prev, attachment]
      );
    },
  });

  const addStartingImage = (image: { name: string; mime: string; url: string }) => {
    const next = sourceAttachment(image);
    setAttachments((prev) => (prev.some((item) => item.id === next.id) ? prev : [...prev, next]));
  };
  const [sending, setSending] = useState(false);
  const [operationError, setOperationError] = useState<string | null>(null);
  const [chatOperationError, setChatOperationError] = useState<string | null>(null);

  const {
    chats,
    loading: chatsLoading,
    error: chatsError,
    createChat,
    deleteChat: deleteChatFn,
    archiveChat: archiveChatFn,
    unarchiveChat: unarchiveChatFn,
    refresh: refreshChats,
    renameChat,
    updateTitle: updateListTitle,
  } = useChats({ archived: showArchived });
  const { projects } = useProjects('all');
  const projectNames = useMemo(
    () => Object.fromEntries(projects.map((project) => [project.id, project.name])),
    [projects]
  );
  const arranged = useMemo(
    () => arrangeChats(chats, { groupBy, sort, projectNames }),
    [chats, groupBy, sort, projectNames]
  );

  const {
    chat: activeChat,
    context,
    contextError,
    previewing,
    error: chatError,
    streaming,
    status: chatStatus,
    sendMessage: sendMessageFn,
    cancelGeneration,
    approveTool,
    setAgentEnabled: setAgentEnabledFn,
    setAutoApprove: setAutoApproveFn,
    setAgentSandboxed: setAgentSandboxedFn,
    setReasoningEffort: setReasoningEffortFn,
    setModelName: setModelNameFn,
    setContextTokens: setContextTokensFn,
    setCharacter: setCharacterFn,
    clearCharacter: clearCharacterFn,
    refresh: refreshChat,
    updateTitle,
  } = useChat(selectedChatId, updateListTitle, {
    content: messageInput,
    metadata: attachmentMetadata(attachments.filter(isSendable)),
  });

  const {
    results: searchResults,
    searching,
    error: searchError,
    search,
    clear: clearSearch,
  } = useChatSearch();

  const { sources: workspaceSources } = useSources({ activeOnly: true });
  const {
    sources: attachedSources,
    loading: attachedLoading,
    error: attachedError,
    setAttached: setAttachedSources,
  } = useChatSources(selectedChatId);
  const {
    providers: chatProviders,
    loading: providersLoading,
    error: providersError,
    choose: chooseProvider,
    refresh: refreshProviders,
  } = useChatProviders(selectedChatId);
  const [changingChats, setChangingChats] = useState<ReadonlySet<string>>(new Set());
  const changingProvider = selectedChatId !== null && changingChats.has(selectedChatId);
  const markChanging = (chatId: string, changing: boolean) => {
    setChangingChats((current) => {
      const next = new Set(current);
      if (changing) next.add(chatId);
      else next.delete(chatId);
      return next;
    });
  };
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  // Only render a conversation that matches the current selection so a
  // previous chat never flashes in the main pane while the next one loads.
  const displayedChat = activeChat?.id === selectedChatId ? activeChat : null;
  const chatOpen = displayedChat !== null;
  const changes = useChatPullRequests(chatOpen ? selectedChatId : null, streaming);
  const tasks = useChatTasks(chatOpen ? selectedChatId : null, streaming);
  const displayedMessages = displayedChat?.messages;
  const media = useMemo(() => galleryItems(displayedMessages ?? []), [displayedMessages]);
  const sources = useMemo(() => chatSources(displayedMessages ?? []), [displayedMessages]);
  const [shownSources, setShownSources] = useState<SourceRequest | null>(null);
  const clearShownSources = useCallback(() => setShownSources(null), []);
  const wasStreaming = useRef(streaming);
  const latestSelectedChatId = useRef(selectedChatId);

  // A chat change resets streaming before the new chat is shown, and loads its providers itself.
  useEffect(() => {
    if (wasStreaming.current === streaming) return;
    wasStreaming.current = streaming;
    if (chatOpen) void refreshProviders();
  }, [streaming, chatOpen, refreshProviders]);

  useEffect(() => {
    latestSelectedChatId.current = selectedChatId;
    setShownSources(null);
    setChatOperationError(null);
    setOperationError(null);
  }, [selectedChatId]);

  const chatOffer = chatProviders ? effectiveOffer(chatProviders) : null;
  const agentChat = chatOffer !== null && isAgentOffer(chatOffer);
  const providerChosen = chatProviders
    ? chatProviders.selected !== null
    : Boolean(displayedChat?.provider);
  const providerDriven = agentChat || providerChosen;
  const chosenSelection = chatProviders?.selected ?? null;
  const chatEfforts = chosenSelection
    ? effortsFor(chosenSelection, chosenSelection.model)
    : chatOffer
      ? effortsFor(chatOffer, displayedChat?.model_name)
      : [];
  const installedForChat =
    displayedChat && !providerDriven
      ? displayedChat.model_name === AUTO_MODEL
        ? {
            tools: models.some((model) => offersAgent(model)),
            reasoning: models.some((model) => chatShowsReasoning({}, model)),
          }
        : findInstalledModel(models, displayedChat.model_name)
      : undefined;
  // An answer to an agent's question is sent as an ordinary user message, so a
  // user message after the assistant message carrying a question card is that
  // answer and settles the card. Nothing on the server records it.
  const lastUserIndex = (displayedChat?.messages ?? []).reduce(
    (last, message, index) =>
      message.role === 'user' && !automated(message.metadata) ? index : last,
    -1
  );
  const chatListsModels =
    !providerChosen ||
    (chatProviders !== null && chatOffer?.provider === chatProviders.default.provider);
  const chatModel = chosenSelection
    ? (chosenSelection.model ?? AUTO_MODEL)
    : !displayedChat
      ? AUTO_MODEL
      : chatProviders
        ? displayedChat.model_name
        : requestedModel(displayedChat);
  const showAgent = displayedChat
    ? agentChat ||
      chatShowsAgent(displayedChat) ||
      (displayedChat.tools !== false &&
        offersAgentOn(chatOffer, chatListsModels ? models : null, chatModel))
    : false;
  const showCharacter = displayedChat ? chatShowsCharacter(displayedChat, installedForChat) : false;
  const showReasoning = !displayedChat
    ? false
    : providerDriven
      ? chatEfforts.length > 0
      : chatShowsReasoning(displayedChat, installedForChat);
  const chatReasoningOptions = providerDriven
    ? reasoningOptions(chatEfforts)
    : REASONING_EFFORT_OPTIONS;
  const chosenOfferFor = (provider: string): ChatProviderOffer | null =>
    workspaceProviders?.providers.find((offer) => offer.provider === provider) ?? null;
  const newChatOfferFor = (provider: string): ChatProviderOffer | null => {
    const fallback = workspaceProviders?.default;
    return chosenOfferFor(provider) ?? (fallback && isAgentOffer(fallback) ? fallback : null);
  };
  const chosenNewOffer = chosenOfferFor(newChatProvider);
  const newChatOffer = newChatOfferFor(newChatProvider);
  const newChatEfforts = newChatOffer ? effortsFor(newChatOffer, newChatModel) : null;
  const newChatEffort = newChatEfforts
    ? keepEffort(newChatReasoning, newChatEfforts)
    : newChatReasoning;
  const selectedNewModel = findInstalledModel(models, newChatModel);
  const autoNewChat = newChatModel === AUTO_MODEL;
  const newChatNamesModel = newChatOffer !== null && asksModelName(newChatOffer);
  const newChatNeedsModel = Boolean(newChatOffer?.requires_model) && !newChatModel.trim();
  const newChatShowsAgent = (offer: ChatProviderOffer | null, model: string): boolean =>
    offersAgentOn(
      offer,
      offer === null || offer.provider === workspaceProviders?.default.provider ? models : null,
      model
    );
  const showNewChatAgent = newChatShowsAgent(newChatOffer, newChatModel);
  const showNewChatReasoning = newChatEfforts
    ? newChatEfforts.length > 0
    : autoNewChat
      ? models.some((model) => chatShowsReasoning({}, model))
      : selectedNewModel
        ? chatShowsReasoning({}, selectedNewModel)
        : false;
  const workspaceProvider = workspaceProviders?.default.provider ?? null;
  const showNewChatContext = offersContextOn(
    chosenNewOffer?.provider ?? workspaceProvider,
    workspaceProvider,
    models,
    newChatModel
  );
  const newChatNative = selectedNewModel?.details?.context_length;
  const chatDefaultProvider = chatProviders?.default.provider ?? workspaceProvider;
  const chatRunsOn = chatProviders
    ? (chatProviders.selected ?? chatProviders.default).provider
    : (displayedChat?.provider ?? workspaceProvider);
  const showChatContext =
    displayedChat !== null && offersContextOn(chatRunsOn, chatDefaultProvider, models, chatModel);
  const chatNative = showChatContext
    ? findInstalledModel(models, chatModel)?.details?.context_length
    : undefined;
  const providerName = chatProviderName(chatProviders, displayedChat);
  const modelName = chatModelName(chatProviders, displayedChat);
  const chatEffortName = showReasoning
    ? REASONING_EFFORT_LABELS[displayedChat?.reasoning_effort ?? AUTO_EFFORT]
    : null;
  const chatSettingsSummary = [providerName, modelName, chatEffortName].filter(Boolean).join(' · ');
  const autoApproved = Boolean(displayedChat?.auto_approve);
  const sandboxed = displayedChat?.agent_sandboxed !== false;
  const cliStale = Boolean(providersError) || changingProvider || (streaming && providersLoading);
  const cli = cliStale && chatProviders?.cli === false ? undefined : chatProviders?.cli;
  const showSandbox = Boolean(
    displayedChat?.agent_enabled && (cli === undefined ? !sandboxed : cli)
  );
  const permissionSummary = [
    `auto-approve ${autoApproved ? 'on' : 'off'}`,
    ...(showSandbox
      ? [sandboxed ? 'Zone tools only on' : 'Host tools on (Zone tools only off)']
      : []),
  ].join(', ');
  const meterCapacity = showChatContext
    ? selectedContextTokens(displayedChat?.context_tokens, chatNative)
    : null;

  const scrollToBottom = useCallback(() => {
    if (!stickToBottom.current) {
      return;
    }
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, []);

  const handleMessagesScroll = () => {
    const node = messagesContainerRef.current;
    if (!node) {
      return;
    }
    stickToBottom.current = node.scrollHeight - node.scrollTop - node.clientHeight <= 80;
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: a newly selected chat starts stuck to its bottom
  useEffect(() => {
    stickToBottom.current = true;
  }, [selectedChatId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: keep new messages and generation feedback visible
  useEffect(() => {
    if (linkedMessageId) {
      return;
    }
    scrollToBottom();
  }, [activeChat?.messages, chatError, chatStatus, streaming, linkedMessageId, scrollToBottom]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the scroller mounts with the displayed chat
  useEffect(() => {
    const node = messagesContainerRef.current;
    if (!node) {
      return;
    }
    const observer = new ResizeObserver(() => {
      if (!stickToBottom.current) {
        return;
      }
      node.scrollTo({ top: node.scrollHeight, behavior: 'instant' });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [displayedChat?.id]);

  useEffect(() => {
    if (!linkedMessageId || !displayedChat) return;
    const node = document.getElementById(`chat-message-${linkedMessageId}`);
    node?.scrollIntoView({ block: 'center' });
  }, [linkedMessageId, displayedChat]);

  useEffect(() => {
    const id = searchParams.get('id');
    setSelectedChatId((current) => (current === id ? current : id));
  }, [searchParams]);

  const selectChat = (chatId: string, messageId?: string) => {
    setSelectedChatId(chatId);
    const next = new URLSearchParams();
    next.set('id', chatId);
    if (messageId) {
      next.set('message', messageId);
    }
    setSearchParams(next, { replace: true });
  };

  const clearSelectedChat = () => {
    setSelectedChatId(null);
    setSearchParams({}, { replace: true });
  };

  const resetNewChat = () => {
    setNewChatProvider(DEFAULT_PROVIDER);
    setNewChatModel(AUTO_MODEL);
    setNewChatAgent(false);
    setNewChatAutoApprove(false);
    setNewChatReasoning(AUTO_EFFORT);
    setNewChatProject(NO_PROJECT);
    setNewChatOffline(false);
    setNewChatContext(String(defaultContextTokens()));
  };

  const workspaceId = currentWorkspace?.id ?? null;
  const [newChatWorkspace, setNewChatWorkspace] = useState(workspaceId);
  if (newChatWorkspace !== workspaceId) {
    setNewChatWorkspace(workspaceId);
    resetNewChat();
  }

  const openNewChat = () => {
    resetNewChat();
    setOperationError(null);
    setShowNewChatModal(true);
    void refreshWorkspaceProviders();
  };

  const keepNewChatAgent = (offer: ChatProviderOffer | null, model: string) => {
    if (newChatShowsAgent(offer, model)) return;
    setNewChatAgent(false);
    setNewChatAutoApprove(false);
  };

  const handleNewChatProvider = (provider: string) => {
    const offer = newChatOfferFor(provider);
    const model = offer?.requires_model ? '' : AUTO_MODEL;
    keepNewChatAgent(offer, model);
    setNewChatProvider(provider);
    setNewChatModel(model);
    setNewChatReasoning(
      keepEffort(
        newChatEffort,
        offer ? effortsFor(offer, model) : REASONING_EFFORT_OPTIONS.map((option) => option.value)
      )
    );
    setNewChatContext(String(defaultContextTokens()));
  };

  const handleNewChatModel = (name: string) => {
    setNewChatModel(name);
    keepNewChatAgent(newChatOffer, name);
    const installed = findInstalledModel(models, name);
    setNewChatContext(String(defaultContextTokens(installed?.details?.context_length)));
    if (newChatOffer) {
      setNewChatReasoning(keepEffort(newChatEffort, effortsFor(newChatOffer, name)));
      return;
    }
    const automatic = name === AUTO_MODEL;
    if (!automatic && !chatShowsReasoning({}, installed)) {
      setNewChatReasoning(AUTO_EFFORT);
    }
  };

  const handleCreateChat = async (event: FormEvent) => {
    event.preventDefault();
    if (!isAuthenticated) {
      setOperationError('You must be logged in to create a chat');
      return;
    }
    if (!currentWorkspace) {
      setOperationError('No workspace selected. Please select or create a workspace first.');
      return;
    }
    if (newChatNeedsModel) {
      setOperationError(NEW_CHAT_MODEL_REQUIRED);
      return;
    }
    const modelName = newChatModel.trim() || AUTO_MODEL;
    const agentEnabled = showNewChatAgent && newChatAgent;
    setOperationError(null);
    try {
      const chat = await createChat({
        workspace_id: currentWorkspace.id,
        title: modelName === AUTO_MODEL ? 'New chat' : `Chat with ${modelName}`,
        automatic_title: true,
        model_name: chosenNewOffer ? AUTO_MODEL : modelName,
        ...(chosenNewOffer
          ? {
              provider: chosenNewOffer.provider,
              provider_model: modelName === AUTO_MODEL ? null : modelName,
            }
          : {}),
        agent_enabled: agentEnabled,
        auto_approve: agentEnabled && newChatAutoApprove,
        reasoning_effort:
          showNewChatReasoning && newChatEffort !== AUTO_EFFORT ? newChatEffort : undefined,
        ...(newChatProject !== NO_PROJECT ? { project_id: newChatProject } : {}),
        ...(newChatOffline ? { offline: true } : {}),
        ...(showNewChatContext ? { context_tokens: Number(newChatContext) } : {}),
      });
      setShowNewChatModal(false);
      resetNewChat();
      selectChat(chat.id);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : 'Failed to create chat');
    }
  };

  const reportForChat = (chatId: string, message: string) => {
    if (latestSelectedChatId.current === chatId) setChatOperationError(message);
  };

  const handleToggleAgent = async () => {
    if (!isAuthenticated || !displayedChat) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    try {
      await setAgentEnabledFn(!displayedChat.agent_enabled);
    } catch (error) {
      reportForChat(chatId, error instanceof Error ? error.message : 'Failed to change agent mode');
    }
  };

  const handleToggleAutoApprove = async () => {
    if (!isAuthenticated || !displayedChat) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    try {
      await setAutoApproveFn(!displayedChat.auto_approve);
    } catch (error) {
      reportForChat(
        chatId,
        error instanceof Error ? error.message : 'Failed to change auto-approve'
      );
    }
  };

  const handleToggleAgentSandboxed = async () => {
    if (!isAuthenticated || !displayedChat) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    try {
      const sandboxed = displayedChat.agent_sandboxed !== false;
      await setAgentSandboxedFn(!sandboxed);
    } catch (error) {
      reportForChat(
        chatId,
        error instanceof Error ? error.message : 'Failed to change tool access'
      );
    }
  };

  const handleReasoningEffort = async (effort: ReasoningEffort) => {
    if (!isAuthenticated || !displayedChat) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    try {
      await setReasoningEffortFn(effort);
    } catch (error) {
      reportForChat(chatId, error instanceof Error ? error.message : 'Failed to change reasoning');
    }
  };

  const settleEffort = async (efforts: ReasoningEffort[]) => {
    const current = displayedChat?.reasoning_effort ?? AUTO_EFFORT;
    const next = keepEffort(current, efforts);
    if (next !== current) await setReasoningEffortFn(next);
  };

  const offerFor = (provider: AiProvider | null): ChatProviderOffer | undefined => {
    if (!chatProviders) return undefined;
    if (provider === null) return chatProviders.default;
    const { providers, selected } = chatProviders;
    return (
      providers.find((offer) => offer.provider === provider) ??
      (selected?.provider === provider ? selected : undefined)
    );
  };

  const handleChooseProvider = async (
    provider: AiProvider | null,
    model: string | null
  ): Promise<boolean> => {
    if (!isAuthenticated || !displayedChat) return false;
    const chatId = displayedChat.id;
    const modelName = displayedChat.model_name;
    setChatOperationError(null);
    markChanging(chatId, true);
    try {
      await chooseProvider(provider, model);
    } catch (error) {
      reportForChat(
        chatId,
        error instanceof Error ? error.message : 'Failed to change AI provider'
      );
      markChanging(chatId, false);
      return false;
    }
    try {
      const offer = offerFor(provider);
      if (offer) await settleEffort(effortsFor(offer, provider === null ? modelName : model));
    } catch (error) {
      reportForChat(chatId, failure(PROVIDER_EFFORT_RESET_FAILED, error));
    } finally {
      await Promise.all([
        refreshChats(),
        latestSelectedChatId.current === chatId ? refreshChat() : undefined,
      ]);
      markChanging(chatId, false);
    }
    return true;
  };

  const handleProviderModel = async (name: string) => {
    if (!isAuthenticated || !displayedChat || !chatProviders) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    markChanging(chatId, true);
    try {
      await setModelNameFn(name);
    } catch (error) {
      reportForChat(chatId, error instanceof Error ? error.message : 'Failed to change model');
      markChanging(chatId, false);
      return;
    }
    try {
      await settleEffort(effortsFor(chatProviders.default, name));
    } catch (error) {
      reportForChat(chatId, failure(MODEL_EFFORT_RESET_FAILED, error));
    } finally {
      try {
        await Promise.all([
          refreshChats(),
          latestSelectedChatId.current === chatId
            ? Promise.all([refreshProviders(), refreshChat()])
            : undefined,
        ]);
      } finally {
        markChanging(chatId, false);
      }
    }
  };

  const handleContextTokens = async (tokens: number) => {
    if (!isAuthenticated || !displayedChat) return;
    const chatId = displayedChat.id;
    setChatOperationError(null);
    try {
      await setContextTokensFn(tokens);
    } catch (error) {
      reportForChat(
        chatId,
        error instanceof Error ? error.message : 'Failed to change context size'
      );
    }
  };

  const handleIncomingFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    const leftover: File[] = [];
    for (const file of Array.from(files)) {
      if (displayedChat && showCharacter) {
        const chatId = displayedChat.id;
        try {
          const card = await parseCharacterFile(file);
          if (card) {
            if (latestSelectedChatId.current === chatId) setChatOperationError(null);
            await setCharacterFn(card);
            continue;
          }
        } catch (error) {
          reportForChat(
            chatId,
            error instanceof Error ? error.message : 'Failed to attach character'
          );
          continue;
        }
      }
      leftover.push(file);
    }
    const attached = new Set(attachments.map((attachment) => attachment.id));
    attaching.start(leftover.filter((file) => !attached.has(attachmentId(file))));
  };

  const handleSaveCharacter = async (event: FormEvent) => {
    event.preventDefault();
    if (!displayedChat) return;
    const card = parseCharacterText(characterDraft);
    if (!card) {
      setCharacterError('Paste a character card JSON or a system prompt.');
      return;
    }
    setCharacterSaving(true);
    setCharacterError(null);
    try {
      await setCharacterFn(card);
      setShowCharacterModal(false);
      setCharacterDraft('');
    } catch (error) {
      setCharacterError(error instanceof Error ? error.message : 'Failed to save character');
    } finally {
      setCharacterSaving(false);
    }
  };

  const handleCharacterUpload = async (files: FileList | null) => {
    if (!files?.[0] || !displayedChat) return;
    setCharacterSaving(true);
    setCharacterError(null);
    try {
      const card = await parseCharacterFile(files[0]);
      if (!card) {
        setCharacterError('That file is not a character card, JSON card, or text prompt.');
        return;
      }
      await setCharacterFn(card);
      setShowCharacterModal(false);
      setCharacterDraft('');
    } catch (error) {
      setCharacterError(error instanceof Error ? error.message : 'Failed to save character');
    } finally {
      setCharacterSaving(false);
    }
  };

  const handleClearCharacter = async () => {
    if (!displayedChat) return;
    setCharacterSaving(true);
    setCharacterError(null);
    try {
      await clearCharacterFn();
      setShowCharacterModal(false);
      setCharacterDraft('');
    } catch (error) {
      setCharacterError(error instanceof Error ? error.message : 'Failed to remove character');
    } finally {
      setCharacterSaving(false);
    }
  };

  const handleRename = async (event: FormEvent): Promise<void> => {
    event.preventDefault();
    if (!renameId || renamePending.current || !isAuthenticated) return;
    const trimmed = title.trim();
    if (!trimmed) {
      setRenameError('Enter a chat name');
      return;
    }
    renamePending.current = true;
    setRenaming(true);
    setRenameError(null);
    try {
      const updated = await renameChat(renameId, trimmed);
      updateTitle(updated.id, updated.title);
      setRenameId(null);
    } catch (error) {
      setRenameError(error instanceof Error ? error.message : 'Failed to rename chat');
    } finally {
      renamePending.current = false;
      setRenaming(false);
    }
  };

  const handleSendMessage = async (e: FormEvent) => {
    e.preventDefault();
    const sendable = attachments.filter(isSendable);
    if (!isAuthenticated || !displayedChat || sending || streaming) return;
    if (attaching.pending > 0) return;
    if (!messageInput.trim() && sendable.length === 0) return;

    const chatId = displayedChat.id;
    setSending(true);
    setChatOperationError(null);
    stickToBottom.current = true;
    try {
      await sendMessageFn({
        content: buildMessageWithAttachments(messageInput.trim(), sendable),
        metadata: attachmentMetadata(sendable),
      });
      setMessageInput('');
      setAttachments([]);
    } catch (error) {
      reportForChat(chatId, error instanceof Error ? error.message : 'Failed to send message');
    } finally {
      setSending(false);
    }
    try {
      await refreshChats();
    } catch {
      // The message already left; a stale sidebar is not a failed send.
    }
  };

  const handleAnswerQuestions = async (content: string) => {
    if (!isAuthenticated || !displayedChat || sending || streaming || !content.trim()) return;

    const chatId = displayedChat.id;
    setSending(true);
    setChatOperationError(null);
    stickToBottom.current = true;
    try {
      await sendMessageFn({ content });
    } catch (error) {
      reportForChat(chatId, error instanceof Error ? error.message : 'Failed to send message');
    } finally {
      setSending(false);
    }
  };

  const handleArchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await archiveChatFn(chatId);
      if (selectedChatId === chatId) {
        clearSelectedChat();
      }
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : 'Failed to archive chat');
    }
  };

  const handleUnarchiveChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await unarchiveChatFn(chatId);
      if (selectedChatId === chatId) {
        clearSelectedChat();
      }
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : 'Failed to unarchive chat');
    }
  };

  const handleDeleteChat = async (chatId: string) => {
    if (!isAuthenticated) return;
    setOperationError(null);
    try {
      await deleteChatFn(chatId);
      if (selectedChatId === chatId) {
        clearSelectedChat();
      }
      setDeleteConfirm(null);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : 'Failed to delete chat');
    }
  };

  const handleSearch = async (e: FormEvent) => {
    e.preventDefault();
    if (!isAuthenticated || !searchQuery.trim()) return;

    setShowSearchResults(true);
    await search(searchQuery, { limit: 20 });
  };

  const searchResultTitle = (result: ChatSearchResult): string =>
    result.chat_title ||
    chats.find((chat) => chat.id === result.chat_id)?.title ||
    UNTITLED_CHAT_TITLE;

  const handleSearchResultClick = async (result: ChatSearchResult) => {
    selectChat(result.chat_id, result.message_id);
    setShowSearchResults(false);
    setSearchQuery('');
    clearSearch();
  };

  const handleClearSearch = () => {
    setSearchQuery('');
    clearSearch();
    setShowSearchResults(false);
  };

  const handleGroupBy = (value: string) => {
    if (!isChatGroupBy(value)) return;
    setGroupBy(value);
    writeChatGroupBy(value);
  };

  const handleSort = (value: string) => {
    if (!isChatSort(value)) return;
    setSort(value);
    writeChatSort(value);
  };

  const openRename = (chat: Chat) => {
    setRenameId(chat.id);
    setTitle(chat.title);
    setRenameError(null);
  };

  const renderChatItem = (chat: Chat) => (
    <div
      key={chat.id}
      className={`chat-item ${selectedChatId === chat.id ? 'active' : ''}`}
      onClick={() => selectChat(chat.id)}
      onKeyDown={(e) => e.target === e.currentTarget && e.key === 'Enter' && selectChat(chat.id)}
      role="button"
      tabIndex={0}
    >
      <div className="chat-item-content">
        <span className="chat-title">{chat.title}</span>
        <span className="chat-meta">
          <span className="chat-meta-model">{modelLabel(requestedModel(chat))}</span>
          <span className="chat-meta-time">· {formatDate(chat.updated_at)}</span>
        </span>
      </div>
      <div className="chat-item-actions">
        <ChatMenu
          chat={chat}
          archived={showArchived}
          onRename={openRename}
          onArchive={(target) => handleArchiveChat(target.id)}
          onUnarchive={(target) => handleUnarchiveChat(target.id)}
          onDelete={(target) => setDeleteConfirm(target.id)}
        />
      </div>
    </div>
  );

  return (
    <div className={`page page--workspace chats-page ${selectedChatId ? 'has-chat' : ''}`}>
      <SubNav
        className="chats-sidebar"
        storageKey="manager_subnav_chats"
        label="Resize chat list"
        defaultWidthRem={18}
      >
        <PageBar title="Chats" className="chats-sidebar-header">
          <Tabs
            value={showArchived ? 'archived' : 'active'}
            onValueChange={(v) => setShowArchived(v === 'archived')}
            className="chats-filter"
          >
            <TabsList>
              <TabsTrigger value="active" disabled={showSearchResults}>
                Active
              </TabsTrigger>
              <TabsTrigger value="archived" disabled={showSearchResults}>
                Archived
              </TabsTrigger>
            </TabsList>
          </Tabs>
          <button
            type="button"
            className="btn-icon"
            aria-label="New chat"
            title="New chat"
            onClick={openNewChat}
          >
            <PlusIcon />
          </button>
        </PageBar>

        <form className="chat-search" onSubmit={handleSearch}>
          <svg
            className="chat-search-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            width="16"
            height="16"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="7" />
            <path d="M21 21l-4.3-4.3" />
          </svg>
          <input
            type="text"
            placeholder="Search messages..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="chat-search-input"
            data-testid="chat-search-input"
          />
          {searchQuery && (
            <button
              type="button"
              className="chat-search-clear"
              onClick={handleClearSearch}
              aria-label="Clear search"
              data-testid="clear-search-btn"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                width="16"
                height="16"
              >
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          )}
        </form>

        {!showSearchResults && chats.length > 0 && (
          <div className="chats-arrange">
            <Select
              compact
              value={groupBy}
              onValueChange={handleGroupBy}
              aria-label="Group chats"
              options={[...CHAT_GROUP_OPTIONS]}
            />
            <Select
              compact
              value={sort}
              onValueChange={handleSort}
              aria-label="Sort chats"
              options={[...CHAT_SORT_OPTIONS]}
            />
          </div>
        )}

        {operationError && !showNewChatModal && (
          <div className="chats-error" role="alert">
            {operationError}
          </div>
        )}

        {showSearchResults ? (
          searching ? (
            <div className="chats-loading">
              <span className="spinner" /> Searching...
            </div>
          ) : searchError ? (
            <div className="chats-error">{searchError}</div>
          ) : searchResults.length === 0 ? (
            <EmptyState
              icon={
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <circle cx="11" cy="11" r="7" />
                  <path d="M21 21l-4.3-4.3" />
                </svg>
              }
              title="No messages match"
              description="Try another search"
              action={
                <Button variant="secondary" onClick={handleClearSearch}>
                  Clear search
                </Button>
              }
            />
          ) : (
            <div className="chats-list" data-testid="search-results-list">
              {searchResults.map((result) => (
                <div
                  key={result.message_id}
                  className="search-result-item"
                  onClick={() => handleSearchResultClick(result)}
                  onKeyDown={(e) => e.key === 'Enter' && handleSearchResultClick(result)}
                  role="button"
                  tabIndex={0}
                  data-testid="search-result-item"
                >
                  <div className="search-result-header">
                    <span className="search-result-chat">{searchResultTitle(result)}</span>
                    <span className="search-result-date">{formatDate(result.created_at)}</span>
                  </div>
                  <div className="search-result-snippet">{toPlainText(result.snippet)}</div>
                </div>
              ))}
            </div>
          )
        ) : chatsLoading ? (
          <div className="chats-loading">
            <span className="spinner" /> Loading...
          </div>
        ) : chatsError ? (
          <div className="chats-error">{chatsError}</div>
        ) : chats.length === 0 ? (
          <EmptyState
            icon={
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                <path d="M8 10h.01M12 10h.01M16 10h.01M9 16H5a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v8a2 2 0 01-2 2h-5l-5 4z" />
              </svg>
            }
            title={showArchived ? 'No archived chats' : 'No chats yet'}
            description={
              showArchived
                ? 'Your archived conversations will appear here'
                : 'Start a new conversation to get started'
            }
            action={!showArchived ? <Button onClick={openNewChat}>New chat</Button> : undefined}
          />
        ) : (
          <div className="chats-list">
            {arranged.map((group) => (
              <div
                key={group.key}
                className="chat-group"
                role="group"
                aria-label={group.label ?? undefined}
              >
                {group.label ? <h2 className="chat-group-header">{group.label}</h2> : null}
                {group.chats.map(renderChatItem)}
              </div>
            ))}
          </div>
        )}
      </SubNav>

      <div className="chats-main">
        {selectedChatId && !displayedChat && (
          <Button
            variant="ghost"
            size="sm"
            className="chat-back chat-back-state"
            onClick={() => setSelectedChatId(null)}
          >
            Back to chats
          </Button>
        )}
        {displayedChat ? (
          <>
            <div className="chat-header">
              <Button
                variant="ghost"
                size="sm"
                className="chat-back"
                onClick={() => setSelectedChatId(null)}
              >
                Back to chats
              </Button>
              <div className="chat-header-info">
                <h3>{displayedChat.title}</h3>
                {displayedChat.purpose === 'project_planner' && (
                  <span className="chat-purpose" data-testid="chat-purpose">
                    Project planner
                    {displayedChat.project_id && (
                      <>
                        {' · '}
                        <Link to={`/projects?id=${displayedChat.project_id}`}>Open project</Link>
                      </>
                    )}
                  </span>
                )}
                {displayedChat.purpose === 'project_updates' && (
                  <span className="chat-purpose" data-testid="chat-purpose">
                    Project updates
                    {displayedChat.project_id && (
                      <>
                        {' · '}
                        <Link to={`/projects?id=${displayedChat.project_id}`}>Open project</Link>
                      </>
                    )}
                  </span>
                )}
                {displayedChat.project_id &&
                  displayedChat.purpose !== 'project_planner' &&
                  displayedChat.purpose !== 'project_updates' && (
                    <span className="chat-purpose" data-testid="chat-project">
                      <Link to={`/projects?id=${displayedChat.project_id}`}>
                        {projectNames[displayedChat.project_id] ?? 'Open project'}
                      </Link>
                    </span>
                  )}
                {displayedChat.offline && (
                  <span className="chat-purpose" data-testid="chat-offline">
                    Offline
                  </span>
                )}
              </div>
              <div className="chat-header-actions">
                {providersError && (
                  <p
                    className="chat-provider-error"
                    role="alert"
                    data-testid="chat-providers-error"
                  >
                    {providersError}
                  </p>
                )}
                <ProviderError providers={chatProviders} />
                {chatOperationError && (
                  <p
                    className="chat-provider-error"
                    role="alert"
                    data-testid="chat-operation-error"
                  >
                    {chatOperationError}
                  </p>
                )}
                <HeaderPopover
                  key={`settings-${displayedChat.id}`}
                  className="chat-settings-trigger"
                  testId="chat-settings"
                  label="Model settings"
                  triggerLabel={`Model settings: ${chatSettingsSummary}`}
                  title={chatSettingsSummary}
                  disabled={!chatProviders && !showChatContext && !showReasoning}
                  trigger={
                    <>
                      {providerName && (
                        <>
                          <span className="chat-settings-provider">{providerName}</span>
                          <span className="chat-settings-separator" aria-hidden="true">
                            ·
                          </span>
                        </>
                      )}
                      <span className="chat-model" title={modelName}>
                        {shortModelName(modelName)}
                      </span>
                      {chatEffortName && (
                        <>
                          <span className="chat-settings-separator" aria-hidden="true">
                            ·
                          </span>
                          <span className="chat-settings-effort">{chatEffortName}</span>
                        </>
                      )}
                    </>
                  }
                >
                  {chatProviders && (
                    <ProviderPicker
                      key={displayedChat.id}
                      providers={chatProviders}
                      modelName={displayedChat.model_name}
                      disabled={changingProvider}
                      onChoose={handleChooseProvider}
                      onModel={(name) => {
                        void handleProviderModel(name);
                      }}
                    />
                  )}
                  {showChatContext && (
                    <HeaderSelect
                      className="context-tokens"
                      label="Context"
                      ariaLabel="Context size"
                      testId="context-tokens"
                      title="How much context this local model loads. Larger windows use more RAM."
                      value={String(
                        selectedContextTokens(displayedChat.context_tokens, chatNative)
                      )}
                      options={contextTokenOptions(chatNative)}
                      onValueChange={(value) => {
                        void handleContextTokens(Number(value));
                      }}
                    />
                  )}
                  {showReasoning && (
                    <HeaderSelect
                      className="reasoning-effort"
                      label="Reasoning"
                      ariaLabel="Reasoning effort"
                      testId="reasoning-effort"
                      title="How much the model thinks before answering. Auto matches the request."
                      value={displayedChat.reasoning_effort ?? AUTO_EFFORT}
                      options={chatReasoningOptions}
                      onValueChange={(value) => {
                        void handleReasoningEffort(value as ReasoningEffort);
                      }}
                    />
                  )}
                </HeaderPopover>
                {showCharacter && (
                  <button
                    type="button"
                    className="agent-toggle"
                    onClick={() => {
                      setCharacterError(null);
                      setCharacterDraft('');
                      setShowCharacterModal(true);
                    }}
                    aria-pressed={Boolean(displayedChat.character)}
                    title={
                      displayedChat.character
                        ? `Character: ${displayedChat.character.name}. Custom models use this persona instead of Zone's assistant prompt.`
                        : 'Attach a character card or system prompt for models that expect a persona'
                    }
                    data-testid="character-toggle"
                  >
                    {displayedChat.character?.name ?? 'Character'}
                  </button>
                )}
                {(showAgent || displayedChat.agent_enabled) && (
                  <div className="agent-controls" role="group" aria-label="Agent">
                    {showAgent && (
                      <button
                        type="button"
                        className="agent-toggle"
                        onClick={handleToggleAgent}
                        aria-pressed={displayedChat.agent_enabled}
                        title={
                          displayedChat.agent_enabled
                            ? 'Agent mode on: replies can search this workspace, run server commands, and read or write server files'
                            : 'Agent mode off: replies come straight from the model'
                        }
                        data-testid="agent-toggle"
                      >
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.75"
                          width="14"
                          height="14"
                          aria-hidden="true"
                        >
                          <path d="M12 3v2M12 19v2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M3 12h2M19 12h2M5.6 18.4L7 17M17 7l1.4-1.4" />
                          <circle cx="12" cy="12" r="3.5" />
                        </svg>
                        Agent
                      </button>
                    )}
                    {displayedChat.agent_enabled && (
                      <HeaderPopover
                        key={`permissions-${displayedChat.id}`}
                        className="agent-toggle agent-permissions-trigger"
                        testId="agent-permissions"
                        label="Agent permissions"
                        triggerLabel={`Agent permissions: ${permissionSummary}`}
                        title={`Agent permissions: ${permissionSummary}`}
                        trigger={
                          <AgentPermissionSummary
                            autoApproved={autoApproved}
                            hostTools={showSandbox && !sandboxed}
                          />
                        }
                      >
                        <AgentPermission
                          name="Auto-approve"
                          hint={
                            autoApproved
                              ? 'File writes and shell commands run without asking'
                              : 'File writes and shell commands wait for confirmation'
                          }
                          title={
                            autoApproved
                              ? 'Auto-approve on: file writes and shell commands run without asking'
                              : 'Auto-approve off: file writes and shell commands wait for confirmation'
                          }
                          enabled={autoApproved}
                          testId="auto-approve-toggle"
                          onToggle={handleToggleAutoApprove}
                        />
                        {showSandbox && (
                          <AgentPermission
                            name="Zone tools only"
                            hint={
                              sandboxed
                                ? 'The agent can call only the tools Zone gives it'
                                : 'The agent can also use its own tools on this host, which Zone never sees'
                            }
                            title={
                              sandboxed
                                ? 'Zone tools only on: the agent can call nothing but the tools Zone gives it, so Zone sees and gates every call'
                                : 'Zone tools only off: the agent can also read, write and run commands on this host with its own tools, which Zone never sees and cannot gate'
                            }
                            enabled={sandboxed}
                            warning={!sandboxed}
                            testId="agent-sandbox-toggle"
                            onToggle={handleToggleAgentSandboxed}
                          />
                        )}
                      </HeaderPopover>
                    )}
                  </div>
                )}
              </div>
            </div>

            <div className="chat-body">
              <div className="chat-thread">
                <div
                  className="messages-container"
                  ref={messagesContainerRef}
                  onScroll={handleMessagesScroll}
                >
                  {displayedChat.messages.length === 0 ? (
                    <EmptyState
                      className="messages-empty"
                      icon={
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.5"
                          aria-hidden="true"
                        >
                          <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                        </svg>
                      }
                      title="No messages yet"
                      description="Send a message to start the conversation"
                    />
                  ) : (
                    displayedChat.messages.map((message, index) => {
                      const images = imageAttachments(message.metadata);
                      const videos = videoAttachments(message.metadata);
                      const audios = audioAttachments(message.metadata);
                      const toolCalls = message.metadata?.tool_calls ?? [];
                      const citations = message.metadata?.citations ?? [];
                      const cited = sources.replies.get(message.id);
                      const receipts = message.metadata?.action_receipts ?? [];
                      const handovers =
                        message.role === 'assistant' ? (message.metadata?.handovers ?? []) : [];
                      const memoryUsed =
                        message.role === 'assistant' && Boolean(message.metadata?.memory_used);
                      const links = message.role === 'assistant' ? 'citations' : 'all';
                      const linked = linkedMessageId === message.id;
                      // The turn still being written. Anything it detached dies with
                      // it, so this is also the only turn whose job cards can claim
                      // to be watching something that is still running.
                      const live = streaming && message.id === displayedChat.messages.at(-1)?.id;
                      const body = message.content.trim();
                      const awaitingAnswer = /^\[waiting for your answer\]$/i.test(body);
                      return (
                        <div
                          key={message.id}
                          id={`chat-message-${message.id}`}
                          className={`message message-${message.role}${linked ? ' message--linked' : ''}`}
                        >
                          <div className="message-header">
                            {message.role === 'system' && (
                              <span className="message-role">System</span>
                            )}
                            <AutomationBadge metadata={message.metadata} />
                            <MemoryBadge used={memoryUsed} />
                            <span className="message-time">{formatDate(message.created_at)}</span>
                          </div>
                          {images.length > 0 && (
                            <div
                              className="message-images"
                              data-image-count={Math.min(images.length, 3)}
                            >
                              {images.map((a, index) => {
                                const starting = attachments.some(
                                  (item) => item.id === `source:${a.url}`
                                );
                                return (
                                  <div key={a.url} className="message-image-frame">
                                    <AuthenticatedImage
                                      src={a.url}
                                      alt={a.name || `Generated image ${index + 1}`}
                                      openLabel={`Open ${a.name || `image ${index + 1}`} full size`}
                                      linkClassName="message-image-link"
                                      title="Open full size"
                                      loading="lazy"
                                      decoding="async"
                                      data-testid="message-image"
                                    />
                                    <button
                                      type="button"
                                      className="btn btn-secondary btn-sm message-image-use"
                                      onClick={() => addStartingImage(a)}
                                      disabled={starting}
                                    >
                                      {starting
                                        ? 'Added as starting image'
                                        : 'Use as starting image'}
                                    </button>
                                  </div>
                                );
                              })}
                            </div>
                          )}
                          {videos.length > 0 && (
                            <div className="message-videos">
                              {videos.map((a, index) => (
                                <AuthenticatedVideo
                                  key={a.url}
                                  src={a.url}
                                  label={a.name || `Generated video ${index + 1}`}
                                  data-testid="message-video"
                                />
                              ))}
                            </div>
                          )}
                          {audios.length > 0 && (
                            <div className="message-audios">
                              {audios.map((a, index) => (
                                <AuthenticatedAudio
                                  key={a.url}
                                  src={a.url}
                                  label={a.name || `Generated audio ${index + 1}`}
                                  data-testid="message-audio"
                                />
                              ))}
                            </div>
                          )}
                          <Activity
                            reasoning={message.metadata?.reasoning}
                            calls={toolCalls}
                            live={live}
                            answered={index < lastUserIndex}
                            onDecide={approveTool}
                            onAnswer={handleAnswerQuestions}
                          />
                          {receipts.length > 0 && <ActionReceipts receipts={receipts} />}
                          {(body || handovers.length > 0) && !awaitingAnswer ? (
                            <div className="message-content">
                              {splitAtHandovers(message.content, handovers).map((part) =>
                                part.kind === 'text' ? (
                                  <MessageContent
                                    key={`text-${part.offset}`}
                                    content={part.text}
                                    links={links}
                                    citations={citations}
                                    breaks={message.role === 'user'}
                                    onShowSource={(citation) =>
                                      setShownSources({
                                        messageId: message.id,
                                        key: observationKey(citation),
                                      })
                                    }
                                  />
                                ) : (
                                  <HandoverNotice
                                    key={`handover-${part.handover.at}-${part.handover.to}`}
                                    handover={part.handover}
                                    agentChanged={part.agentChanged}
                                  />
                                )
                              )}
                            </div>
                          ) : null}
                          {cited && (
                            <SourcesButton
                              count={cited.length}
                              onClick={() => setShownSources({ messageId: message.id, key: null })}
                            />
                          )}
                        </div>
                      );
                    })
                  )}
                  {chatError ? (
                    <div className="chats-error" role="alert">
                      {chatError}
                    </div>
                  ) : null}
                  {streaming || chatStatus ? (
                    <Generation key={selectedChatId} status={chatStatus} />
                  ) : null}
                  <div ref={messagesEndRef} />
                </div>

                <div className="chat-composer">
                  <form
                    className={`message-form${isDragging ? ' is-dragging' : ''}`}
                    onSubmit={handleSendMessage}
                    onDragOver={(e) => {
                      e.preventDefault();
                      setIsDragging(true);
                    }}
                    onDragLeave={(e) => {
                      if (e.currentTarget.contains(e.relatedTarget as Node)) return;
                      setIsDragging(false);
                    }}
                    onDrop={(e) => {
                      e.preventDefault();
                      setIsDragging(false);
                      void handleIncomingFiles(e.dataTransfer.files);
                    }}
                  >
                    {attachments.length + attaching.transfers.length > 0 && (
                      <div className="message-attachments">
                        {attaching.transfers.map((transfer) => (
                          <TransferChip
                            key={transfer.id}
                            transfer={transfer}
                            onCancel={attaching.cancel}
                            onRetry={attaching.retry}
                          />
                        ))}
                        {attachments.map((attachment) => (
                          <span
                            key={attachment.id}
                            className={`attachment-chip${attachment.rejected ? ' is-rejected' : ''}${attachment.url ? ' has-thumb' : ''}`}
                          >
                            {attachment.url ? (
                              isProtectedArtifactUrl(attachment.url) ? (
                                <AuthenticatedImage
                                  src={attachment.url}
                                  alt=""
                                  className="attachment-chip-thumb"
                                  linked={false}
                                  compact
                                />
                              ) : (
                                <img
                                  className="attachment-chip-thumb"
                                  src={attachment.url}
                                  alt=""
                                />
                              )
                            ) : null}
                            <span className="attachment-chip-name">{attachment.name}</span>
                            <span className="attachment-chip-size">
                              {attachment.rejected ? (
                                <span className="attachment-chip-note">{attachment.rejected}</span>
                              ) : isStartingImage(attachment) ? (
                                <span className="attachment-chip-source">Starting image</span>
                              ) : (
                                formatBytes(attachment.size)
                              )}
                            </span>
                            <button
                              type="button"
                              className="attachment-chip-remove"
                              onClick={() => removeAttachment(attachment.id)}
                              aria-label={`Remove ${attachment.name}`}
                            >
                              ×
                            </button>
                          </span>
                        ))}
                      </div>
                    )}
                    {attaching.pending > 0 ? (
                      <p className="message-form-hint" role="status">
                        {attaching.pending === 1
                          ? 'Send is ready once the file finishes attaching.'
                          : `Send is ready once ${attaching.pending} files finish attaching.`}
                      </p>
                    ) : null}
                    {attachments.some((attachment) => attachment.url && !attachment.rejected) ? (
                      <p className="message-form-hint">
                        Ask to generate, edit, remove an object, change the setting, or animate and
                        this image will be the starting point.
                      </p>
                    ) : null}

                    <div className="message-form-row">
                      <input
                        ref={fileInputRef}
                        type="file"
                        multiple
                        hidden
                        onChange={(e) => {
                          void handleIncomingFiles(e.target.files);
                          e.target.value = '';
                        }}
                      />
                      <button
                        type="button"
                        className="btn-icon"
                        onClick={() => fileInputRef.current?.click()}
                        aria-label="Attach files"
                        title="Attach files"
                      >
                        <svg
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.75"
                          width="16"
                          height="16"
                          aria-hidden="true"
                        >
                          <rect x="4.5" y="4.5" width="15" height="15" rx="3.5" />
                          <path d="M12 8.75v6.5M8.75 12h6.5" strokeLinecap="square" />
                        </svg>
                      </button>
                      <textarea
                        placeholder="Type a message, or drop a file..."
                        value={messageInput}
                        onChange={(e) => setMessageInput(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter' && !e.shiftKey) {
                            e.preventDefault();
                            handleSendMessage(e);
                          }
                        }}
                        onPaste={(e) => {
                          if (e.clipboardData.files.length > 0) {
                            e.preventDefault();
                            void handleIncomingFiles(e.clipboardData.files);
                          }
                        }}
                        disabled={sending}
                        rows={1}
                      />
                      {streaming ? (
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={cancelGeneration}
                        >
                          Stop
                        </Button>
                      ) : (
                        <Button
                          type="submit"
                          variant="primary"
                          size="sm"
                          loading={sending}
                          disabled={
                            attaching.pending > 0 ||
                            (!messageInput.trim() && !attachments.some(isSendable))
                          }
                          title={
                            attaching.pending > 0
                              ? 'Wait for attachments to finish before sending'
                              : undefined
                          }
                        >
                          Send
                        </Button>
                      )}
                    </div>
                  </form>
                  <div className="chat-composer-footer">
                    <ChatSources
                      attached={attachedSources}
                      available={workspaceSources}
                      loading={attachedLoading}
                      error={attachedError}
                      onChange={setAttachedSources}
                    />
                    <ContextUsage
                      usage={context ?? null}
                      error={contextError}
                      previewing={previewing}
                      capacity={meterCapacity}
                    />
                  </div>
                </div>
              </div>
              <ChatPanel
                chatId={displayedChat.id}
                userId={user?.id ?? null}
                changes={changes}
                tasks={tasks}
                media={media}
                sources={sources}
                shownSources={shownSources}
                onSourcesShown={clearShownSources}
                onShowMessage={(messageId) => selectChat(displayedChat.id, messageId)}
              />
            </div>
          </>
        ) : selectedChatId && chatError ? (
          <div className="chat-placeholder">
            <div className="chats-error" role="alert">
              {chatError}
            </div>
          </div>
        ) : selectedChatId ? (
          <div className="chat-placeholder">
            <div className="chats-loading">
              <span className="spinner" /> Loading...
            </div>
          </div>
        ) : (
          <div className="chat-placeholder">
            <EmptyState
              icon={
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
                  <path d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z" />
                </svg>
              }
              title="Select a chat to start"
              description="Choose an existing conversation or create a new one"
              action={
                <Button variant="primary" onClick={openNewChat}>
                  New chat
                </Button>
              }
            />
          </div>
        )}
      </div>

      {/* New Chat Modal */}
      <Modal isOpen={showNewChatModal} onClose={() => setShowNewChatModal(false)} title="New Chat">
        <form className="ui-form" onSubmit={handleCreateChat}>
          {operationError && <div className="modal-error">{operationError}</div>}
          {workspaceProvidersError && (
            <div className="modal-error" role="alert" data-testid="new-chat-providers-error">
              {workspaceProvidersError}
            </div>
          )}
          {workspaceProviders && (
            <Select
              label="Provider"
              value={chosenNewOffer ? newChatProvider : DEFAULT_PROVIDER}
              onChange={(event: React.ChangeEvent<HTMLSelectElement>) => {
                handleNewChatProvider(event.target.value);
              }}
              helpText="Default follows the workspace AI settings."
              options={providerChoices(workspaceProviders.default, workspaceProviders.providers)}
            />
          )}
          {newChatOffer && newChatNamesModel ? (
            <Input
              label="Model name"
              value={newChatModel}
              onChange={(event: React.ChangeEvent<HTMLInputElement>) => {
                handleNewChatModel(event.target.value);
              }}
              placeholder={MODEL_NAME_PLACEHOLDER}
              helpText={`AI Settings save no model for ${newChatOffer.label}, so name one it serves.`}
              spellCheck={false}
              autoComplete="off"
            />
          ) : (
            <Select
              label="Select Model"
              value={newChatModel}
              onChange={(event: React.ChangeEvent<HTMLSelectElement>) => {
                handleNewChatModel(event.target.value);
              }}
              placeholder={MODEL_PLACEHOLDER}
              helpText={
                !newChatOffer
                  ? 'Automatic picks a chat, image, video or audio model from the message.'
                  : newChatOffer.requires_model
                    ? `${newChatOffer.label} needs a model chosen for this chat.`
                    : `Choose Automatic to let ${newChatOffer.label} pick its own model.`
              }
              error={newChatOffer ? undefined : (modelsError ?? undefined)}
              options={
                newChatOffer
                  ? modelSelectOptions(newChatOffer, newChatModel)
                  : [
                      { value: AUTO_MODEL, label: 'Automatic' },
                      ...models
                        .filter((model) => model.completion !== false)
                        .map((model) => ({ value: model.name, label: model.name })),
                    ]
              }
            />
          )}
          {projects.length > 0 && (
            <Select
              label="Project"
              value={newChatProject}
              onChange={(event: React.ChangeEvent<HTMLSelectElement>) => {
                setNewChatProject(event.target.value);
              }}
              helpText="Optional. Group this chat with a project."
              options={[
                { value: NO_PROJECT, label: 'No project' },
                ...projects.map((project) => ({ value: project.id, label: project.name })),
              ]}
            />
          )}
          {showNewChatContext && (
            <Select
              label="Context"
              value={newChatContext}
              onChange={(event: React.ChangeEvent<HTMLSelectElement>) => {
                setNewChatContext(event.target.value);
              }}
              helpText={
                newChatNative
                  ? `Larger windows use more RAM. This model supports up to ${contextTokenOptions(newChatNative).at(-1)?.label ?? 'its native size'}.`
                  : 'Larger windows use more RAM on this machine.'
              }
              options={contextTokenOptions(newChatNative)}
            />
          )}
          <Checkbox
            label="Offline"
            helpText="Never use web search, page fetch, or curl/wget. Connected sources still work."
            checked={newChatOffline}
            onCheckedChange={setNewChatOffline}
          />
          {showNewChatAgent && (
            <Checkbox
              label="Agent mode"
              helpText="Search workspace content, use connected GitHub data and run tools when asked. Needs a tool-calling model."
              checked={newChatAgent}
              onCheckedChange={(checked) => {
                setNewChatAgent(checked);
                if (!checked) setNewChatAutoApprove(false);
              }}
            />
          )}
          {showNewChatAgent && newChatAgent && (
            <Checkbox
              label="Auto-approve writes and commands"
              helpText="Skip the confirmation prompt for write_file, apply_patch, run_command and run_shell. You can change this later on the chat."
              checked={newChatAutoApprove}
              onCheckedChange={setNewChatAutoApprove}
            />
          )}
          {showNewChatReasoning && (
            <Select
              label="Reasoning"
              value={newChatEffort}
              onChange={(event: React.ChangeEvent<HTMLSelectElement>) => {
                setNewChatReasoning(event.target.value as ReasoningEffort);
              }}
              helpText={
                newChatOffer && isAgentOffer(newChatOffer)
                  ? `Auto lets ${newChatOffer.label} choose how hard to think.`
                  : 'Auto chooses low, medium, or high from the request. Off skips thinking.'
              }
              options={newChatEfforts ? reasoningOptions(newChatEfforts) : REASONING_EFFORT_OPTIONS}
            />
          )}
          <div className="modal-actions">
            <Button variant="secondary" onClick={() => setShowNewChatModal(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={newChatNeedsModel}>
              Create Chat
            </Button>
          </div>
        </form>
      </Modal>

      <Modal
        isOpen={renameId !== null}
        onClose={() => {
          if (!renamePending.current) setRenameId(null);
        }}
        title="Rename chat"
      >
        <form onSubmit={handleRename}>
          <label className="form-label" htmlFor="chat-name">
            Chat name
          </label>
          <input
            id="chat-name"
            type="text"
            className="form-input"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            disabled={renaming}
            aria-invalid={renameError ? true : undefined}
            aria-describedby={renameError ? 'rename-error' : undefined}
          />
          {renameError && (
            <div id="rename-error" className="chats-error" role="alert">
              {renameError}
            </div>
          )}
          <div className="modal-actions">
            <Button variant="secondary" disabled={renaming} onClick={() => setRenameId(null)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={renaming || !title.trim()}>
              {renaming ? 'Saving…' : 'Save name'}
            </Button>
          </div>
        </form>
      </Modal>

      <Modal
        isOpen={showCharacterModal}
        onClose={() => {
          if (!characterSaving) setShowCharacterModal(false);
        }}
        title={displayedChat?.character ? displayedChat.character.name : 'Character'}
      >
        <form className="ui-form" onSubmit={handleSaveCharacter}>
          <p className="help-text">
            Custom models can use this card as the persona instead of Zone&apos;s assistant prompt.
            Drop a JSON or PNG card, or paste the system prompt. Turn Agent off unless you also want
            tools.
          </p>
          {displayedChat?.character?.description ? (
            <p className="help-text">{displayedChat.character.description}</p>
          ) : null}
          <input
            ref={characterFileRef}
            type="file"
            accept=".json,.png,.txt,.md,application/json,image/png,text/plain"
            hidden
            onChange={(event) => {
              void handleCharacterUpload(event.target.files);
              event.target.value = '';
            }}
          />
          <label htmlFor="character-draft">Card JSON or system prompt</label>
          <textarea
            id="character-draft"
            className="form-input"
            rows={6}
            value={characterDraft}
            onChange={(event) => setCharacterDraft(event.target.value)}
            placeholder='{"spec":"chara_card_v2","data":{"name":"...", "first_mes":"..."}}'
            disabled={characterSaving}
          />
          {characterError && (
            <div className="chats-error" role="alert">
              {characterError}
            </div>
          )}
          <div className="modal-actions">
            <Button
              type="button"
              variant="secondary"
              disabled={characterSaving}
              onClick={() => characterFileRef.current?.click()}
            >
              Upload card
            </Button>
            {displayedChat?.character && (
              <Button
                type="button"
                variant="secondary"
                disabled={characterSaving}
                onClick={() => void handleClearCharacter()}
              >
                Remove
              </Button>
            )}
            <Button
              type="submit"
              variant="primary"
              disabled={characterSaving || !characterDraft.trim()}
            >
              {characterSaving ? 'Saving…' : 'Save character'}
            </Button>
          </div>
        </form>
      </Modal>

      {/* Delete Confirmation Modal */}
      <Modal
        isOpen={deleteConfirm !== null}
        onClose={() => setDeleteConfirm(null)}
        title="Delete Chat"
        size="sm"
      >
        <p>Are you sure you want to delete this chat? This action cannot be undone.</p>
        <div className="modal-actions">
          <Button variant="secondary" onClick={() => setDeleteConfirm(null)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => deleteConfirm && handleDeleteChat(deleteConfirm)}
          >
            Delete
          </Button>
        </div>
      </Modal>
    </div>
  );
}

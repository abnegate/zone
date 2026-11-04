import { ActionMenu, type ActionMenuItem } from '@zone/ui';
import type { Chat } from '../types';

export interface ChatMenuProps {
  chat: Chat;
  archived: boolean;
  onRename: (chat: Chat) => void;
  onArchive: (chat: Chat) => void;
  onUnarchive: (chat: Chat) => void;
  onDelete: (chat: Chat) => void;
}

function Icon({ path }: { path: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

const RENAME_ICON = 'm16 3 5 5-12 12H4v-5L16 3ZM14 5l5 5';
const ARCHIVE_ICON = 'M21 8v13H3V8M1 3h22v5H1zM10 12h4';
const UNARCHIVE_ICON = 'M21 8v13H3V8M1 3h22v5H1zM12 18v-6M9 15l3-3 3 3';
const DELETE_ICON =
  'M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16';

export function ChatMenu({
  chat,
  archived,
  onRename,
  onArchive,
  onUnarchive,
  onDelete,
}: ChatMenuProps) {
  const items: ActionMenuItem[] = [
    { label: 'Rename', icon: <Icon path={RENAME_ICON} />, onSelect: () => onRename(chat) },
    archived
      ? {
          label: 'Unarchive',
          icon: <Icon path={UNARCHIVE_ICON} />,
          onSelect: () => onUnarchive(chat),
        }
      : { label: 'Archive', icon: <Icon path={ARCHIVE_ICON} />, onSelect: () => onArchive(chat) },
    {
      label: 'Delete',
      icon: <Icon path={DELETE_ICON} />,
      destructive: true,
      onSelect: () => onDelete(chat),
    },
  ];

  return <ActionMenu label={`Actions for ${chat.title}`} items={items} />;
}

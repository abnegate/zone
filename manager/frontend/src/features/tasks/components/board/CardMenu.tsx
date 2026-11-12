import { ActionMenu, type ActionMenuItem } from '@zone/ui';
import { MenuIcon } from '../../../chats/components/MenuIcon';
import { TOGGLE } from '../../gesture';
import type { BoardActions, Task } from '../../types';

export interface CardMenuProps {
  task: Task;
  selected: boolean;
  actions: BoardActions;
}

const RUN_ICON = 'M7 4.5v15l12-7.5-12-7.5Z';
const RENAME_ICON = 'm16 3 5 5-12 12H4v-5L16 3ZM14 5l5 5';
const DESELECT_ICON = 'M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Z';
const SELECT_ICON = `${DESELECT_ICON}m3 8 3 3 5-6`;
const DELETE_ICON =
  'M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16';

export function CardMenu({ task, selected, actions }: CardMenuProps) {
  const items: ActionMenuItem[] = [
    { label: 'Run', icon: <MenuIcon path={RUN_ICON} />, onSelect: () => actions.run(task) },
    {
      label: 'Rename',
      icon: <MenuIcon path={RENAME_ICON} />,
      onSelect: () => actions.rename(task),
    },
    {
      label: selected ? 'Deselect' : 'Select',
      icon: <MenuIcon path={selected ? DESELECT_ICON : SELECT_ICON} />,
      onSelect: () => actions.select(task, TOGGLE),
    },
    {
      label: 'Delete',
      icon: <MenuIcon path={DELETE_ICON} />,
      destructive: true,
      onSelect: () => actions.remove(task),
    },
  ];

  return <ActionMenu label={`Actions for ${task.title}`} items={items} />;
}

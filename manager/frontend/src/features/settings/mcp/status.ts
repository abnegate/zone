import type { BadgeProps } from '@zone/ui';
import { Status } from './constants';
import type { McpServerStatus } from './types';

export interface StatusBadge {
  variant: NonNullable<BadgeProps['variant']>;
  label: string;
}

export const STATUS_BADGES: Readonly<Record<McpServerStatus, StatusBadge>> = {
  [Status.Connected]: { variant: 'success', label: 'Connected' },
  [Status.Failed]: { variant: 'destructive', label: 'Failed' },
  [Status.Connecting]: { variant: 'neutral', label: 'Connecting' },
  [Status.Disabled]: { variant: 'outline', label: 'Disabled' },
};

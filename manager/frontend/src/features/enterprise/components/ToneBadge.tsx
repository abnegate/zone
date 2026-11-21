import { Badge, type BadgeProps } from '@zone/ui';
import type { ReactNode } from 'react';
import { Tone } from '../status';

const VARIANTS: Readonly<Record<Tone, BadgeProps['variant']>> = {
  [Tone.Neutral]: 'neutral',
  [Tone.Success]: 'success',
  [Tone.Warning]: 'warning',
  [Tone.Error]: 'destructive',
};

interface ToneBadgeProps {
  tone: Tone;
  children: ReactNode;
}

export function ToneBadge({ tone, children }: ToneBadgeProps) {
  return <Badge variant={VARIANTS[tone]}>{children}</Badge>;
}

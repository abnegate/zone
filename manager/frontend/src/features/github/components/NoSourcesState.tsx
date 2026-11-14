import { buttonVariants, EmptyState } from '@zone/ui';
import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { SOURCES_PATH } from '../../settings';

export const NO_SOURCES_TITLE = 'No GitHub sources';

export const CONNECT_SOURCE = 'Connect a source';

export interface NoSourcesStateProps {
  icon: ReactNode;
  description: string;
  className?: string;
}

export function NoSourcesState({ icon, description, className }: NoSourcesStateProps) {
  return (
    <EmptyState
      className={className}
      icon={icon}
      title={NO_SOURCES_TITLE}
      description={description}
      action={
        <Link to={SOURCES_PATH} className={buttonVariants({ variant: 'secondary' })}>
          {CONNECT_SOURCE}
        </Link>
      }
    />
  );
}

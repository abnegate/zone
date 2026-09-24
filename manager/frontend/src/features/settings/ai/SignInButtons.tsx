import { Button } from '@zone/ui';
import type { ReactNode } from 'react';
import type { SignInAction } from './types';

interface SignInButtonsProps {
  full: boolean;
  usable: boolean;
  busy: SignInAction | null;
  /** What to do next while the link can still be used. */
  next?: ReactNode;
  /** Another way to sign in, beside full access. */
  alternative?: ReactNode;
  onRestart: () => void;
  onFullAccess: () => void;
  onCancel: () => void;
}

/** The buttons under a Claude sign-in: the next step, or starting again once the link is spent. */
export function SignInButtons({
  full,
  usable,
  busy,
  next,
  alternative,
  onRestart,
  onFullAccess,
  onCancel,
}: SignInButtonsProps) {
  return (
    <div className="agent-sign-in-buttons">
      {usable ? (
        next
      ) : (
        <Button size="sm" onClick={onRestart} loading={busy === 'restart'} disabled={busy !== null}>
          Start again
        </Button>
      )}
      {!full && (
        <Button
          size="sm"
          variant={usable ? 'ghost' : 'secondary'}
          onClick={onFullAccess}
          loading={busy === 'full'}
          disabled={busy !== null}
        >
          Try again with full access
        </Button>
      )}
      {alternative}
      <Button
        size="sm"
        variant="ghost"
        onClick={onCancel}
        loading={busy === 'cancel'}
        disabled={busy !== null}
      >
        Cancel
      </Button>
    </div>
  );
}

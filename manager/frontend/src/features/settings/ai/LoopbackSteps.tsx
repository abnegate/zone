import { Button, buttonVariants } from '@zone/ui';
import { format } from 'date-fns';
import type { RefObject } from 'react';
import type { SignInAction } from './types';

interface LoopbackStepsProps {
  url: string;
  full: boolean;
  expiresAt: string;
  usable: boolean;
  busy: SignInAction | null;
  entry: RefObject<HTMLAnchorElement | null>;
  onRestart: () => void;
  onFullAccess: () => void;
  onPaste: () => void;
  onCancel: () => void;
}

export function LoopbackSteps({
  url,
  full,
  expiresAt,
  usable,
  busy,
  entry,
  onRestart,
  onFullAccess,
  onPaste,
  onCancel,
}: LoopbackStepsProps) {
  const fullAccess = !full && (
    <Button
      size="sm"
      variant={usable ? 'ghost' : 'secondary'}
      onClick={onFullAccess}
      loading={busy === 'full'}
      disabled={busy !== null}
    >
      Try again with full access
    </Button>
  );
  const paste = (
    <Button
      size="sm"
      variant="link"
      onClick={onPaste}
      loading={busy === 'paste'}
      disabled={busy !== null}
    >
      Paste a code instead
    </Button>
  );
  const cancel = (
    <Button size="sm" variant="ghost" onClick={onCancel} disabled={busy !== null}>
      Cancel
    </Button>
  );

  if (!usable) {
    return (
      <div className="agent-sign-in-buttons">
        <Button size="sm" onClick={onRestart} loading={busy === 'restart'} disabled={busy !== null}>
          Start again
        </Button>
        {fullAccess}
        {paste}
        {cancel}
      </div>
    );
  }

  const expiry = format(new Date(expiresAt), 'p');

  return (
    <div className="agent-sign-in-loopback">
      <div className="agent-sign-in-step">
        <span>Open claude.com, sign in, and approve access.</span>
        <a
          ref={entry}
          className={buttonVariants({ variant: 'secondary', size: 'sm' })}
          href={url}
          target="_blank"
          rel="noopener noreferrer"
        >
          Open claude.com
        </a>
      </div>
      <p className="form-hint">
        {full
          ? `This link asks for full access to your Claude account. It expires at ${expiry}.`
          : `The link expires at ${expiry}.`}
      </p>
      <div className="agent-sign-in-buttons">
        {fullAccess}
        {paste}
        {cancel}
      </div>
    </div>
  );
}

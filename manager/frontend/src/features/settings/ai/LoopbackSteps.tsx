import { Button } from '@zone/ui';
import type { RefObject } from 'react';
import { ClaudeLink } from './ClaudeLink';
import { SignInButtons } from './SignInButtons';
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
  const buttons = (
    <SignInButtons
      full={full}
      usable={usable}
      busy={busy}
      alternative={
        <Button
          size="sm"
          variant="link"
          onClick={onPaste}
          loading={busy === 'paste'}
          disabled={busy !== null}
        >
          Paste a code instead
        </Button>
      }
      onRestart={onRestart}
      onFullAccess={onFullAccess}
      onCancel={onCancel}
    />
  );

  if (!usable) return buttons;

  return (
    <div className="agent-sign-in-loopback">
      <ClaudeLink url={url} full={full} expiresAt={expiresAt} entry={entry} />
      <p className="form-hint">
        Approve in this browser, on the machine Zone runs on: claude.com sends it back to Zone at
        localhost. From any other machine, paste a code instead.
      </p>
      {buttons}
    </div>
  );
}

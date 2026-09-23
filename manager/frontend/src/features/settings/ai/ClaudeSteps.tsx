import { Button, buttonVariants } from '@zone/ui';
import { format } from 'date-fns';
import { type KeyboardEvent, type RefObject, useId } from 'react';
import type { SignInAction } from './types';

interface ClaudeStepsProps {
  url: string;
  full: boolean;
  expiresAt: string;
  usable: boolean;
  code: string;
  codeError: string | null;
  busy: SignInAction | null;
  entry: RefObject<HTMLInputElement | null>;
  onCodeChange: (code: string) => void;
  onSubmit: () => void;
  onRestart: () => void;
  onFullAccess: () => void;
  onCancel: () => void;
}

export function ClaudeSteps({
  url,
  full,
  expiresAt,
  usable,
  code,
  codeError,
  busy,
  entry,
  onCodeChange,
  onSubmit,
  onRestart,
  onFullAccess,
  onCancel,
}: ClaudeStepsProps) {
  const codeId = useId();
  const hintId = useId();
  const errorId = useId();

  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    onSubmit();
  };

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
        {cancel}
      </div>
    );
  }

  const expiry = format(new Date(expiresAt), 'p');

  return (
    <ol className="agent-sign-in-steps">
      <li>
        <div className="agent-sign-in-step">
          <span>Open claude.com, sign in, and approve access.</span>
          <a
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
      </li>
      <li>
        <div className="form-group">
          <label htmlFor={codeId}>Code from claude.com</label>
          <input
            ref={entry}
            id={codeId}
            type="text"
            className="form-input"
            value={code}
            onChange={(event) => onCodeChange(event.target.value)}
            onKeyDown={submitOnEnter}
            placeholder="code#state, or the address of the page showing it"
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            aria-describedby={codeError ? `${hintId} ${errorId}` : hintId}
            aria-invalid={codeError ? true : undefined}
          />
          <p id={hintId} className="form-hint">
            {full
              ? 'Paste the code claude.com shows.'
              : 'Paste the code claude.com shows. If claude.com refuses the request, try again with full access.'}
          </p>
          {codeError && (
            <p id={errorId} className="field-error" role="alert">
              {codeError}
            </p>
          )}
        </div>
        <div className="agent-sign-in-buttons">
          <Button
            size="sm"
            onClick={onSubmit}
            loading={busy === 'submit'}
            disabled={busy !== null || !code.trim()}
          >
            Submit code
          </Button>
          {fullAccess}
          {cancel}
        </div>
      </li>
    </ol>
  );
}

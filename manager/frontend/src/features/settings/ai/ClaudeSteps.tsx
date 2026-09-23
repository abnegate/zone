import { Button, buttonVariants } from '@zone/ui';
import { type KeyboardEvent, useId } from 'react';
import type { SignInAction } from './types';

interface ClaudeStepsProps {
  url: string;
  full: boolean;
  code: string;
  busy: SignInAction | null;
  rejected: boolean;
  onCodeChange: (code: string) => void;
  onSubmit: () => void;
  onFullAccess: () => void;
  onCancel: () => void;
}

export function ClaudeSteps({
  url,
  full,
  code,
  busy,
  rejected,
  onCodeChange,
  onSubmit,
  onFullAccess,
  onCancel,
}: ClaudeStepsProps) {
  const codeId = useId();

  const submitOnEnter = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    onSubmit();
  };

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
        {full && (
          <p className="form-hint">This link asks for full access to your Claude account.</p>
        )}
      </li>
      <li>
        <div className="form-group">
          <label htmlFor={codeId}>Code from claude.com</label>
          <input
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
          />
          <p className="form-hint">
            Paste the code claude.com shows. If claude.com refuses the request, try again with full
            access.
          </p>
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
          <Button
            size="sm"
            variant={rejected ? 'secondary' : 'ghost'}
            onClick={onFullAccess}
            loading={busy === 'full'}
            disabled={busy !== null}
          >
            Try again with full access
          </Button>
          <Button size="sm" variant="ghost" onClick={onCancel} disabled={busy !== null}>
            Cancel
          </Button>
        </div>
      </li>
    </ol>
  );
}

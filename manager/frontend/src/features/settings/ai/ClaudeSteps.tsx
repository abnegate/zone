import { Button } from '@zone/ui';
import { type KeyboardEvent, type ReactNode, type RefObject, useId } from 'react';
import { ClaudeLink } from './ClaudeLink';
import { SignInButtons } from './SignInButtons';
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

  const buttons = (next?: ReactNode) => (
    <SignInButtons
      full={full}
      usable={usable}
      busy={busy}
      next={next}
      onRestart={onRestart}
      onFullAccess={onFullAccess}
      onCancel={onCancel}
    />
  );

  if (!usable) return buttons();

  return (
    <ol className="agent-sign-in-steps">
      <li>
        <ClaudeLink url={url} full={full} expiresAt={expiresAt} />
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
        {buttons(
          <Button
            size="sm"
            onClick={onSubmit}
            loading={busy === 'submit'}
            disabled={busy !== null || !code.trim()}
          >
            Submit code
          </Button>
        )}
      </li>
    </ol>
  );
}

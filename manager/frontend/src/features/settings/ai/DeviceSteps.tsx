import { Button } from '@zone/ui';
import { format } from 'date-fns';
import type { RefObject } from 'react';
import type { DevicePrompt } from './schemas';
import type { SignInAction } from './types';

interface DeviceStepsProps {
  prompt: DevicePrompt | null;
  account: string;
  busy: SignInAction | null;
  entry: RefObject<HTMLElement | null>;
  onCancel: () => void;
}

export function DeviceSteps({ prompt, account, busy, entry, onCancel }: DeviceStepsProps) {
  const expiresAt = prompt ? format(new Date(prompt.expires_at), 'p') : null;

  return (
    <>
      {prompt && (
        <ol className="agent-sign-in-steps">
          <li>
            <div className="agent-sign-in-step">
              <span>
                Open{' '}
                <a
                  className="agent-sign-in-link"
                  href={prompt.verification_url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {prompt.verification_url.replace(/^https?:\/\//, '')}
                </a>{' '}
                and sign in with {account}.
              </span>
            </div>
          </li>
          {prompt.user_code && (
            <li>
              <div className="agent-sign-in-step">
                <span>Enter this one-time code:</span>
                <code ref={entry} className="agent-sign-in-code" tabIndex={-1}>
                  {prompt.user_code}
                </code>
              </div>
              <p className="form-hint">
                {expiresAt && `Expires at ${expiresAt}. `}
                Only enter it if you started this sign-in here. If a website or someone else gave
                you this code, cancel.
              </p>
            </li>
          )}
        </ol>
      )}
      <div className="agent-sign-in-buttons">
        <Button
          size="sm"
          variant="ghost"
          onClick={onCancel}
          loading={busy === 'signOut'}
          disabled={busy !== null}
        >
          Cancel
        </Button>
      </div>
    </>
  );
}

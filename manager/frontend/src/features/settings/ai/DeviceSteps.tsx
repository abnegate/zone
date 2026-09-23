import { Button } from '@zone/ui';
import type { DevicePrompt } from './schemas';
import type { SignInAction } from './types';

interface DeviceStepsProps {
  prompt: DevicePrompt | null;
  account: string;
  busy: SignInAction | null;
  onCancel: () => void;
}

function formatTime(value: string): string | null {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

export function DeviceSteps({ prompt, account, busy, onCancel }: DeviceStepsProps) {
  const expiresAt = prompt ? formatTime(prompt.expires_at) : null;

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
                <code className="agent-sign-in-code">{prompt.user_code}</code>
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

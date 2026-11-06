import { Button, Input } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { ApiError } from '../../../../api/ApiError';
import type { GitHubAppRegistration } from '../../../github';
import { submitManifest } from '../../../github/utils/submitManifest';

const ConnectTarget = {
  Account: 'account',
  Organization: 'organization',
} as const;

type ConnectTarget = (typeof ConnectTarget)[keyof typeof ConnectTarget];

const TARGET_LABELS: Readonly<Record<ConnectTarget, string>> = {
  [ConnectTarget.Account]: 'Your account',
  [ConnectTarget.Organization]: 'An organization',
};

const REGISTRATION_ERRORS: Readonly<Partial<Record<number, string>>> = {
  400: "That isn't a valid GitHub organization login.",
  409: 'Zone is already connected to this GitHub account.',
};

function registrationError(error: unknown): string {
  if (error instanceof ApiError) {
    return REGISTRATION_ERRORS[error.status] ?? error.message;
  }
  return error instanceof Error ? error.message : 'Failed to connect GitHub';
}

interface GitHubConnectFormProps {
  onRegister: (owner: string | null) => Promise<GitHubAppRegistration>;
  onCancel?: () => void;
}

export function GitHubConnectForm({ onRegister, onCancel }: GitHubConnectFormProps) {
  const targetName = useId();
  const loginId = useId();
  const [target, setTarget] = useState<ConnectTarget>(ConnectTarget.Account);
  const [login, setLogin] = useState('');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const owner = target === ConnectTarget.Organization ? login.trim() : null;
  const blank = owner === '';

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (pending || blank) {
      return;
    }
    setPending(true);
    setError(null);
    try {
      submitManifest(await onRegister(owner));
    } catch (failure) {
      setError(registrationError(failure));
      setPending(false);
    }
  };

  return (
    <form className="settings-card github-connect" onSubmit={submit}>
      <fieldset className="github-connect-target" disabled={pending}>
        <legend className="github-connect-legend">Connect to</legend>
        <div className="radio-group">
          {Object.values(ConnectTarget).map((value) => (
            <label key={value} className="radio-option">
              <input
                type="radio"
                name={targetName}
                value={value}
                checked={target === value}
                onChange={() => setTarget(value)}
              />
              <span className="radio-label">{TARGET_LABELS[value]}</span>
            </label>
          ))}
        </div>
      </fieldset>
      {target === ConnectTarget.Organization && (
        <Input
          id={loginId}
          label="Organization login"
          helpText="You need permission to create GitHub Apps in this organization."
          value={login}
          autoComplete="off"
          spellCheck={false}
          disabled={pending}
          onChange={(event) => setLogin(event.target.value)}
        />
      )}
      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      <div className="github-connect-actions">
        {onCancel && (
          <Button variant="ghost" disabled={pending} onClick={onCancel}>
            Cancel
          </Button>
        )}
        <Button type="submit" disabled={blank} loading={pending}>
          Connect GitHub
        </Button>
      </div>
    </form>
  );
}

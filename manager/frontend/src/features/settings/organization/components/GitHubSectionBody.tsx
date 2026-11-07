import { Button } from '@zone/ui';
import type { RefObject } from 'react';
import type { GitHubApp, GitHubAppRegistration, GitHubInstallation } from '../../../github';
import { GitHubAccountRow } from './GitHubAccountRow';
import { GitHubConnectForm } from './GitHubConnectForm';

interface GitHubSectionBodyProps {
  loading: boolean;
  error: string | null;
  fetching: boolean;
  canManage: boolean;
  apps: GitHubApp[];
  installationsOf: (app: GitHubApp) => GitHubInstallation[];
  installingId: string | null;
  addingAccount: boolean;
  connectAnotherRef: RefObject<HTMLButtonElement | null>;
  onRetry: () => void;
  onRegister: (owner: string | null) => Promise<GitHubAppRegistration>;
  onFinishInstalling: (app: GitHubApp) => void;
  onRemove: (app: GitHubApp) => void;
  onConnectAnother: () => void;
  onCancelConnect: () => void;
}

export function GitHubSectionBody({
  loading,
  error,
  fetching,
  canManage,
  apps,
  installationsOf,
  installingId,
  addingAccount,
  connectAnotherRef,
  onRetry,
  onRegister,
  onFinishInstalling,
  onRemove,
  onConnectAnother,
  onCancelConnect,
}: GitHubSectionBodyProps) {
  if (loading) {
    return (
      <div className="loading-state" role="status">
        Loading GitHub connection…
      </div>
    );
  }
  if (error !== null) {
    return (
      <div className="github-notice github-notice-error github-notice-inline">
        <span role="alert">{error}</span>
        <Button variant="ghost" size="sm" loading={fetching} onClick={onRetry}>
          Retry
        </Button>
      </div>
    );
  }

  const connected = apps.length > 0;
  const connectingAnother = addingAccount && connected;

  return (
    <>
      {!canManage && (
        <p className="github-read-only">Only organization admins can change GitHub settings.</p>
      )}
      {!canManage && !connected && <p className="github-empty">No GitHub account is connected.</p>}
      {connected && (
        <ul className="github-accounts" aria-label="Connected GitHub accounts">
          {apps.map((app) => (
            <GitHubAccountRow
              key={app.id}
              app={app}
              installations={installationsOf(app)}
              canManage={canManage}
              installing={installingId === app.id}
              onFinishInstalling={() => onFinishInstalling(app)}
              onRemove={() => onRemove(app)}
            />
          ))}
        </ul>
      )}
      {canManage && (!connected || addingAccount) && (
        <GitHubConnectForm
          focusOnMount={connectingAnother}
          onRegister={onRegister}
          onCancel={connectingAnother ? onCancelConnect : undefined}
        />
      )}
      {canManage && connected && !addingAccount && (
        <div className="github-section-actions">
          <Button ref={connectAnotherRef} variant="secondary" size="sm" onClick={onConnectAnother}>
            Connect another account
          </Button>
        </div>
      )}
    </>
  );
}

import { Button } from '@zone/ui';
import { useId, useState } from 'react';
import { githubApi } from '../../../api/github';
import { GITHUB_SETTINGS_PATH } from '../../github/callback';
import type { GitHubConfig, Source, UpdateSourceRequest } from '../types';
import { GitHubSourceState, reachableRepository, resolveGitHubSourceState } from '../utils';

const REPOSITORY_SEARCH_LIMIT = 100;

const ConnectAction = {
  Reconnect: 'Reconnect',
  SwitchToApp: 'Switch to GitHub App',
} as const;

type ConnectAction = (typeof ConnectAction)[keyof typeof ConnectAction];

const TOKEN_REMOVAL_HINT = 'Your access token will be removed.';

interface GitHubSourceConnectionProps {
  source: Source;
  organizationId: string | null;
  canManage: boolean;
  updateSource: (request: UpdateSourceRequest) => Promise<Source>;
  onSaved: () => Promise<void> | void;
  onError: (message: string | null) => void;
}

function connectionSummary(state: GitHubSourceState, login: string | null): string {
  switch (state) {
    case GitHubSourceState.App:
      return login ? `Connected through GitHub App (${login})` : 'Connected through GitHub App';
    case GitHubSourceState.Revoked:
      return login ? `GitHub access for ${login} was revoked.` : 'GitHub access was revoked.';
    case GitHubSourceState.Suspended:
      return login ? `GitHub access for ${login} is suspended.` : 'GitHub access is suspended.';
    case GitHubSourceState.Token:
      return 'Connected with an access token.';
    case GitHubSourceState.Disconnected:
      return 'Not connected to GitHub.';
  }
}

function connectAction(state: GitHubSourceState): ConnectAction | null {
  switch (state) {
    case GitHubSourceState.App:
      return null;
    case GitHubSourceState.Token:
      return ConnectAction.SwitchToApp;
    default:
      return ConnectAction.Reconnect;
  }
}

function connectionHint(state: GitHubSourceState): string | null {
  return state === GitHubSourceState.Token ? TOKEN_REMOVAL_HINT : null;
}

function repositoryFullName(source: Source): string {
  const { owner, repo } = source.config as GitHubConfig;
  return `${owner}/${repo}`;
}

export function GitHubSourceConnection({
  source,
  organizationId,
  canManage,
  updateSource,
  onSaved,
  onError,
}: GitHubSourceConnectionProps) {
  const [connecting, setConnecting] = useState(false);
  const [unreachable, setUnreachable] = useState(false);
  const hintId = useId();

  const state = resolveGitHubSourceState(source);
  const action = connectAction(state);
  const hint = connectionHint(state);
  const fullName = repositoryFullName(source);

  const handleConnect = async () => {
    setUnreachable(false);
    onError(null);
    if (organizationId === null) {
      setUnreachable(true);
      return;
    }
    setConnecting(true);
    try {
      const response = await githubApi.listRepositories(organizationId, {
        query: fullName,
        perPage: REPOSITORY_SEARCH_LIMIT,
      });
      const repository = reachableRepository(response, fullName);
      if (!repository) {
        setUnreachable(true);
        return;
      }
      await updateSource({ github_installation_id: repository.installation_id });
      await onSaved();
    } catch (error) {
      onError(error instanceof Error ? error.message : 'Failed to connect the source to GitHub');
    } finally {
      setConnecting(false);
    }
  };

  return (
    <section className="source-github-connection" aria-label="GitHub connection">
      <div className="source-github-connection-summary">
        <p>{connectionSummary(state, source.github_account_login ?? null)}</p>
        {action && (
          <div className="source-github-connection-action">
            <Button
              variant="secondary"
              size="sm"
              onClick={handleConnect}
              loading={connecting}
              aria-describedby={hint ? hintId : undefined}
            >
              {action}
            </Button>
            {hint && (
              <span id={hintId} className="source-github-connection-hint">
                {hint}
              </span>
            )}
          </div>
        )}
      </div>
      <div className="source-github-connection-unreachable" role="status">
        {unreachable && (
          <>
            <span>No connected GitHub account can reach {fullName}.</span>{' '}
            {canManage ? (
              <a href={GITHUB_SETTINGS_PATH}>Connect GitHub</a>
            ) : (
              <span>Ask an organization admin to connect GitHub.</span>
            )}
          </>
        )}
      </div>
    </section>
  );
}

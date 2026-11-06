import { Button, buttonVariants } from '@zone/ui';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { type GitHubApp, type GitHubInstallation, useGitHubConnection } from '../../../github';
import { GitHubOutcome, GitHubSearchParameter, isGitHubOutcome } from '../../../github/callback';
import { redirectTo } from '../../../github/utils/redirectTo';
import { GitHubAccountRow } from './GitHubAccountRow';
import { GitHubConnectForm } from './GitHubConnectForm';
import { GitHubRemoveDialog } from './GitHubRemoveDialog';
import './GitHubSection.css';

const OUTCOME_NOTICES: Readonly<Record<GitHubOutcome, string>> = {
  [GitHubOutcome.Installed]: 'GitHub account connected.',
  [GitHubOutcome.Requested]: 'Installation requested. A GitHub organization owner must approve it.',
  [GitHubOutcome.Sync]: 'GitHub accounts refreshed.',
  [GitHubOutcome.Expired]: 'The GitHub connection link expired. Start again.',
  [GitHubOutcome.Failed]: 'GitHub could not finish connecting. Try again.',
};

const NO_INSTALLATIONS: GitHubInstallation[] = [];

function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

function groupByApp(installations: GitHubInstallation[]): Map<string, GitHubInstallation[]> {
  const grouped = new Map<string, GitHubInstallation[]>();
  for (const installation of installations) {
    const group = grouped.get(installation.github_app_id);
    if (group === undefined) {
      grouped.set(installation.github_app_id, [installation]);
    } else {
      group.push(installation);
    }
  }
  for (const group of grouped.values()) {
    group.sort((left, right) => Date.parse(right.created_at) - Date.parse(left.created_at));
  }
  return grouped;
}

interface GitHubSectionProps {
  organizationId: string;
  canManage: boolean;
}

export function GitHubSection({ organizationId, canManage }: GitHubSectionProps) {
  const headingId = useId();
  const {
    apps,
    installations,
    loading,
    error,
    refresh,
    registerApp,
    createInstallation,
    syncInstallations,
    removeInstallation,
    deleteApp,
  } = useGitHubConnection(organizationId);
  const [searchParams, setSearchParams] = useSearchParams();
  const outcome = searchParams.get(GitHubSearchParameter.Outcome);
  const handledOutcome = useRef(false);

  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [addingAccount, setAddingAccount] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [removeTarget, setRemoveTarget] = useState<GitHubApp | null>(null);
  const [removedApps, setRemovedApps] = useState<GitHubApp[]>([]);

  useEffect(() => {
    if (outcome === null || handledOutcome.current) {
      return;
    }
    handledOutcome.current = true;
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete(GitHubSearchParameter.Outcome);
        return next;
      },
      { replace: true }
    );
    if (!isGitHubOutcome(outcome)) {
      return;
    }
    if (outcome !== GitHubOutcome.Sync) {
      setNotice(OUTCOME_NOTICES[outcome]);
      void refresh();
      return;
    }
    if (!canManage) {
      void refresh();
      return;
    }
    syncInstallations().then(
      () => setNotice(OUTCOME_NOTICES[GitHubOutcome.Sync]),
      (syncFailure: unknown) => {
        setFailure(`Couldn't refresh GitHub accounts: ${messageOf(syncFailure, 'Unknown error')}`);
        void refresh();
      }
    );
  }, [outcome, canManage, refresh, syncInstallations, setSearchParams]);

  const installationsByApp = useMemo(() => groupByApp(installations), [installations]);
  const installationsOf = (app: GitHubApp) => installationsByApp.get(app.id) ?? NO_INSTALLATIONS;

  const visibleApps = useMemo(() => {
    const removed = new Set(removedApps.map((app) => app.id));
    return apps.filter((app) => !removed.has(app.id));
  }, [apps, removedApps]);

  const finishInstalling = async (app: GitHubApp) => {
    setInstallingId(app.id);
    setFailure(null);
    try {
      redirectTo(await createInstallation(app.id));
    } catch (installFailure) {
      setFailure(messageOf(installFailure, 'Failed to start the GitHub installation'));
      setInstallingId(null);
    }
  };

  const remove = async (app: GitHubApp, uninstall: boolean) => {
    if (uninstall) {
      for (const installation of installationsOf(app)) {
        if (installation.revoked_at === null) {
          await removeInstallation(installation.id, true);
        }
      }
    }
    await deleteApp(app.id);
    setRemovedApps((current) => [...current, app]);
    setRemoveTarget(null);
  };

  const showConnectForm = canManage && (visibleApps.length === 0 || addingAccount);
  const connectingAnother = addingAccount && visibleApps.length > 0;

  const body = () => {
    if (loading) {
      return (
        <div className="loading-state" role="status">
          Loading GitHub connection…
        </div>
      );
    }
    if (error !== null) {
      return (
        <div className="alert alert-error alert-inline">
          <span role="alert">{error}</span>
          <Button variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      );
    }
    return (
      <>
        {!canManage && (
          <p className="github-read-only">Only organization admins can change GitHub settings.</p>
        )}
        {!canManage && visibleApps.length === 0 && (
          <p className="github-empty">No GitHub account is connected.</p>
        )}
        {visibleApps.length > 0 && (
          <ul className="github-accounts" aria-label="Connected GitHub accounts">
            {visibleApps.map((app) => (
              <GitHubAccountRow
                key={app.id}
                app={app}
                installations={installationsOf(app)}
                canManage={canManage}
                installing={installingId === app.id}
                onFinishInstalling={() => finishInstalling(app)}
                onRemove={() => setRemoveTarget(app)}
              />
            ))}
          </ul>
        )}
        {showConnectForm && (
          <GitHubConnectForm
            onRegister={registerApp}
            onCancel={connectingAnother ? () => setAddingAccount(false) : undefined}
          />
        )}
        {canManage && visibleApps.length > 0 && !addingAccount && (
          <div className="github-section-actions">
            <Button variant="secondary" size="sm" onClick={() => setAddingAccount(true)}>
              Connect another account
            </Button>
          </div>
        )}
      </>
    );
  };

  return (
    <section className="github-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            GitHub
          </h2>
          <p className="section-description">
            Connect GitHub accounts so members can pick repositories without access tokens.
          </p>
        </div>
      </div>
      {notice && (
        <div className="alert alert-success" role="status">
          {notice}
        </div>
      )}
      {failure && (
        <div className="alert alert-error" role="alert">
          {failure}
        </div>
      )}
      {removedApps.map((app) => (
        <div key={app.id} className="alert alert-warning alert-inline">
          <span role="status">
            {`${app.owner_login} was removed from Zone. Delete the app on GitHub to finish.`}
          </span>
          <a
            className={buttonVariants({ variant: 'ghost', size: 'sm' })}
            href={app.settings_url}
            target="_blank"
            rel="noopener noreferrer"
          >
            Delete app on GitHub
          </a>
        </div>
      ))}
      {body()}
      {removeTarget && (
        <GitHubRemoveDialog
          key={removeTarget.id}
          app={removeTarget}
          onConfirm={(uninstall) => remove(removeTarget, uninstall)}
          onClose={() => setRemoveTarget(null)}
        />
      )}
    </section>
  );
}

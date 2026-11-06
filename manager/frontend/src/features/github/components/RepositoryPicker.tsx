import { Badge, Button } from '@zone/ui';
import {
  type ChangeEvent,
  type KeyboardEvent,
  type MouseEvent,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useDebouncedValue } from '../../../shared/hooks';
import { GITHUB_SETTINGS_PATH } from '../callback';
import { REPOSITORY_SEARCH_DELAY } from '../constants';
import { useGitHubRepositories } from '../hooks';
import type {
  GitHubRepository,
  GitHubRepositoryInstallation,
  GitHubRepositoryInstallationError,
} from '../types';
import { LockIcon } from './LockIcon';
import './RepositoryPicker.css';

type RepositoryPickerProps = {
  organizationId: string | null;
  canManage: boolean;
  value: GitHubRepository | null;
  onChange: (repository: GitHubRepository | null) => void;
};

type FailingInstallation = GitHubRepositoryInstallation & {
  error: GitHubRepositoryInstallationError;
};

type RepositoryGroup = {
  login: string;
  repositories: GitHubRepository[];
};

const INSTALLATION_ERROR_MESSAGES: Record<
  GitHubRepositoryInstallationError,
  (login: string) => string
> = {
  revoked: (login) => `GitHub access for ${login} was revoked.`,
  suspended: (login) => `GitHub access for ${login} is suspended.`,
  unavailable: (login) => `Couldn't reach GitHub for ${login}.`,
};

function groupByAccount(
  repositories: GitHubRepository[],
  installations: GitHubRepositoryInstallation[]
): RepositoryGroup[] {
  const logins = new Map(
    installations.map((installation) => [installation.id, installation.account_login])
  );
  const groups = new Map<string, GitHubRepository[]>();
  for (const repository of repositories) {
    const login = logins.get(repository.installation_id) ?? repository.owner;
    const group = groups.get(login);
    if (group) {
      group.push(repository);
    } else {
      groups.set(login, [repository]);
    }
  }
  return Array.from(groups, ([login, members]) => ({ login, repositories: members }));
}

function unresolvedFailures(installations: GitHubRepositoryInstallation[]): FailingInstallation[] {
  const healthy = new Set(
    installations
      .filter((installation) => installation.error === null)
      .map((installation) => installation.account_login)
  );
  return installations.filter(
    (installation): installation is FailingInstallation =>
      installation.error !== null && !healthy.has(installation.account_login)
  );
}

function repositoryKey(repository: GitHubRepository): string {
  return `${repository.installation_id}-${repository.id}`;
}

function isSameRepository(
  first: GitHubRepository | null | undefined,
  second: GitHubRepository
): boolean {
  return first != null && repositoryKey(first) === repositoryKey(second);
}

function keepInputFocused(event: MouseEvent) {
  event.preventDefault();
}

export function RepositoryPicker({
  organizationId,
  canManage,
  value,
  onChange,
}: RepositoryPickerProps) {
  const inputId = useId();
  const listboxId = useId();
  const containerRef = useRef<HTMLDivElement>(null);
  const [text, setText] = useState(value?.full_name ?? '');
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const [shownValue, setShownValue] = useState(value);

  if (value !== shownValue) {
    setShownValue(value);
    if (value !== null) {
      setText(value.full_name);
    }
  }

  const search = useDebouncedValue(query.trim(), REPOSITORY_SEARCH_DELAY);
  const {
    repositories,
    installations,
    loading,
    error,
    hasMore,
    loadingMore,
    loadMore,
    refetch,
    refresh,
  } = useGitHubRepositories(organizationId, search);

  const groups = useMemo(
    () => groupByAccount(repositories, installations),
    [repositories, installations]
  );
  const selectable = useMemo(
    () =>
      groups.flatMap((group) => group.repositories).filter((repository) => !repository.archived),
    [groups]
  );
  const reachable = installations.filter((installation) => installation.error === null);
  const failing = unresolvedFailures(installations);

  const optionId = (repository: GitHubRepository) => `${listboxId}-${repositoryKey(repository)}`;
  const active = activeIndex >= 0 ? selectable[activeIndex] : undefined;
  const showNoResults = !loading && error === null && repositories.length === 0;
  const expanded = open && (repositories.length > 0 || showNoResults || hasMore);
  const activeId = expanded && active ? optionId(active) : undefined;
  const listboxShown = expanded && repositories.length > 0;

  useEffect(() => {
    if (activeId !== undefined) {
      document.getElementById(activeId)?.scrollIntoView?.({ block: 'nearest' });
    }
  }, [activeId]);

  useEffect(() => {
    if (!open) {
      return;
    }
    function handleOutside(event: Event) {
      if (!containerRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setActiveIndex(-1);
      }
    }
    document.addEventListener('mousedown', handleOutside);
    document.addEventListener('focusin', handleOutside);
    return () => {
      document.removeEventListener('mousedown', handleOutside);
      document.removeEventListener('focusin', handleOutside);
    };
  }, [open]);

  const disconnected =
    organizationId === null || (!loading && error === null && installations.length === 0);

  if (disconnected) {
    return (
      <div className="form-group repository-picker">
        <span className="form-label">Repository</span>
        <div className="repository-picker-empty">
          <p>No GitHub account is connected.</p>
          {canManage ? (
            <a className="repository-picker-link" href={GITHUB_SETTINGS_PATH}>
              Connect GitHub
            </a>
          ) : (
            <p>Ask an organization admin to connect GitHub.</p>
          )}
        </div>
      </div>
    );
  }

  function close() {
    setOpen(false);
    setActiveIndex(-1);
  }

  function select(repository: GitHubRepository) {
    if (repository.archived) {
      return;
    }
    onChange(repository);
    setText(repository.full_name);
    setQuery('');
    close();
  }

  function edge(step: number): number {
    if (selectable.length === 0) {
      return -1;
    }
    return step > 0 ? 0 : selectable.length - 1;
  }

  function move(step: number) {
    if (!open || active === undefined) {
      setOpen(true);
      setActiveIndex(edge(step));
      return;
    }
    setActiveIndex((activeIndex + step + selectable.length) % selectable.length);
  }

  function handleInput(event: ChangeEvent<HTMLInputElement>) {
    setText(event.target.value);
    setQuery(event.target.value);
    setOpen(true);
    setActiveIndex(-1);
    if (value !== null) {
      onChange(null);
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        move(-1);
        break;
      case 'Enter':
        if (open) {
          event.preventDefault();
          if (active !== undefined) {
            select(active);
          }
        }
        break;
      case 'Escape':
        if (open) {
          event.preventDefault();
          close();
        }
        break;
    }
  }

  async function refreshList() {
    setRefreshing(true);
    setRefreshFailed(false);
    try {
      await refresh();
    } catch {
      setRefreshFailed(true);
    } finally {
      setRefreshing(false);
    }
  }

  return (
    <div className="form-group repository-picker" ref={containerRef}>
      <label htmlFor={inputId}>Repository</label>
      <div className="repository-picker-field">
        <input
          id={inputId}
          type="text"
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={expanded}
          aria-controls={listboxShown ? listboxId : undefined}
          aria-activedescendant={activeId}
          placeholder="Search repositories"
          autoComplete="off"
          spellCheck={false}
          value={text}
          onChange={handleInput}
          onFocus={() => setOpen(true)}
          onClick={() => setOpen(true)}
          onKeyDown={handleKeyDown}
        />
        {expanded && (
          <div className="repository-picker-popup">
            {listboxShown && (
              <div
                id={listboxId}
                role="listbox"
                aria-label="Repositories"
                onMouseDown={keepInputFocused}
              >
                {groups.map((group) => (
                  <div key={group.login} role="group" aria-label={group.login}>
                    <div className="repository-picker-group-label" aria-hidden="true">
                      {group.login}
                    </div>
                    {group.repositories.map((repository) => (
                      // biome-ignore lint/a11y/useKeyWithClickEvents: the combobox input owns keyboard selection through aria-activedescendant
                      <div
                        key={repositoryKey(repository)}
                        id={optionId(repository)}
                        role="option"
                        aria-selected={isSameRepository(value, repository)}
                        aria-disabled={repository.archived ? true : undefined}
                        className={`repository-picker-option${isSameRepository(active, repository) ? ' active' : ''}`}
                        onClick={() => select(repository)}
                        onMouseEnter={() => {
                          if (!repository.archived) {
                            setActiveIndex(selectable.indexOf(repository));
                          }
                        }}
                      >
                        <span className="repository-picker-name">{repository.full_name}</span>
                        {repository.private && <LockIcon />}
                        {repository.archived && <Badge variant="neutral">Archived</Badge>}
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            )}
            {showNoResults && (
              <p className="repository-picker-message">
                {search === ''
                  ? 'No repositories are available.'
                  : `No repositories match “${search}”.`}
              </p>
            )}
            {hasMore && (
              <div className="repository-picker-more">
                <Button
                  variant="ghost"
                  size="sm"
                  loading={loadingMore}
                  onMouseDown={keepInputFocused}
                  onClick={() => void loadMore()}
                >
                  Load more
                </Button>
              </div>
            )}
          </div>
        )}
      </div>
      <p role="status" className="form-hint repository-picker-live">
        {loading ? 'Loading repositories…' : ''}
      </p>
      {error !== null && (
        <div className="repository-picker-error">
          <span>Couldn't load repositories.</span>
          <Button variant="link" size="sm" onClick={() => void refetch()}>
            Retry
          </Button>
        </div>
      )}
      {failing.map((installation) => (
        <p key={installation.id} className="form-hint repository-picker-warning">
          {INSTALLATION_ERROR_MESSAGES[installation.error](installation.account_login)}
        </p>
      ))}
      <p role="alert" className="field-error repository-picker-live">
        {refreshFailed ? "Couldn't refresh repositories." : ''}
      </p>
      {!loading && error === null && (
        <div className="repository-picker-footer">
          <span>Can't find a repo?</span>
          {reachable.map((installation) => (
            <a
              key={installation.id}
              className="repository-picker-link"
              href={installation.html_url}
              target="_blank"
              rel="noopener noreferrer"
            >
              {reachable.length === 1
                ? 'Adjust access on GitHub'
                : `Adjust access to ${installation.account_login} on GitHub`}
            </a>
          ))}
          <Button variant="link" size="sm" loading={refreshing} onClick={() => void refreshList()}>
            Refresh list
          </Button>
        </div>
      )}
    </div>
  );
}

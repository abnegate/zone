import { Button, EmptyState, Select } from '@zone/ui';
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import PageBar from '../../../shared/components/PageBar/PageBar';
import SubNav from '../../../shared/components/SubNav/SubNav';
import {
  FreshnessSchema,
  Icon,
  NoSourcesState,
  noticesOf,
  REFRESH_PATH,
  SourceNotices,
  type SourceStatus,
  UNAVAILABLE,
} from '../../github';
import {
  PULL_REQUEST_PATH,
  PullRequestDetail,
  PullRequestList,
  PullRequestsSkeleton,
  ReadinessFilter,
  type ReadinessOption,
} from '../components';
import { LOAD_FAILED, usePullRequests } from '../hooks';
import type { PullRequest, Readiness } from '../types';
import {
  authorsOf,
  filterPullRequests,
  groupPullRequests,
  parsePullRequestKey,
  pullRequestKey,
  READINESS_LABELS,
  READINESS_ORDER,
  repositoriesOf,
  samePullRequest,
} from '../utils';
import './PullRequestsPage.css';

export const ID_PARAMETER = 'id';

export const PANE_STORAGE_KEY = 'pull-requests-pane';

const PANE_WIDTH_REM = 20;

const ALL = 'all';

const ANY = '';

const FLOORED_DESCRIPTION = 'pull-requests-floored';

type ReadinessTab = Readiness | typeof ALL;

const TABS: readonly ReadinessOption<ReadinessTab>[] = [
  { value: ALL, label: 'All' },
  ...READINESS_ORDER.map((readiness) => ({ value: readiness, label: READINESS_LABELS[readiness] })),
];

export const STALE = 'Showing cached pull requests because GitHub could not be reached.';

export const PARTIAL = 'Some sources could not be read, so this list may be incomplete.';

export const FLOORED = 'Refreshed moments ago. Try again shortly.';

export const REFRESH_LABEL = 'Refresh pull requests';

export const NO_SOURCES_DESCRIPTION =
  'Connect a GitHub source to this workspace to see its pull requests here.';

const NOTICE_TEXTS = { stale: STALE, partial: PARTIAL };

function filterOptions(values: readonly string[], everything: string) {
  return [{ value: ANY, label: everything }, ...values.map((value) => ({ value, label: value }))];
}

function sourceFresh(sources: readonly SourceStatus[], sourceId: string): boolean {
  const source = sources.find((candidate) => candidate.source_id === sourceId);
  return !!source && source.freshness !== FreshnessSchema.enum.stale;
}

export default function PullRequestsPage() {
  const listing = usePullRequests();
  const [searchParams, setSearchParams] = useSearchParams();
  const requested = searchParams.get(ID_PARAMETER);
  const [readiness, setReadiness] = useState<ReadinessTab>(ALL);
  const [repository, setRepository] = useState(ANY);
  const [author, setAuthor] = useState(ANY);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [remembered, setRemembered] = useState<PullRequest | null>(null);
  const [merges, setMerges] = useState(0);
  const honoured = useRef<string | null>(null);
  const title = useRef<HTMLHeadingElement>(null);

  const { pullRequests, partial, sources } = listing;
  const listed = listing.generatedAt !== null;

  const found = useMemo(
    () =>
      selectedKey === null
        ? null
        : (pullRequests.find((pullRequest) => pullRequestKey(pullRequest) === selectedKey) ?? null),
    [pullRequests, selectedKey]
  );

  if (found && found !== remembered) setRemembered(found);

  const reference = parsePullRequestKey(selectedKey);
  const unread = partial && reference !== null && !sourceFresh(sources, reference.source_id);
  const kept = unread && remembered && selectedKey === pullRequestKey(remembered);
  const selected = found ?? (kept ? remembered : null);

  const writeLink = useCallback(
    (key: string | null) => {
      setSearchParams(
        (current) => {
          if (current.get(ID_PARAMETER) === key) return current;
          const next = new URLSearchParams(current);
          if (key === null) {
            next.delete(ID_PARAMETER);
          } else {
            next.set(ID_PARAMETER, key);
          }
          return next;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  useEffect(() => {
    if (!requested) {
      honoured.current = null;
      return;
    }
    if (honoured.current === requested || !listed) return;
    honoured.current = requested;
    const wanted = parsePullRequestKey(requested);
    const match = wanted
      ? pullRequests.find((pullRequest) => samePullRequest(pullRequest, wanted))
      : undefined;
    if (match) {
      setSelectedKey(pullRequestKey(match));
    } else {
      writeLink(null);
    }
  }, [requested, listed, pullRequests, writeLink]);

  useEffect(() => {
    if (!listed || selectedKey === null || selected) return;
    setSelectedKey(null);
    writeLink(null);
  }, [listed, selectedKey, selected, writeLink]);

  useEffect(() => {
    if (merges === 0) return;
    const timer = setTimeout(() => title.current?.focus());
    return () => clearTimeout(timer);
  }, [merges]);

  const repositories = useMemo(() => repositoriesOf(pullRequests), [pullRequests]);
  const authors = useMemo(() => authorsOf(pullRequests), [pullRequests]);
  const groups = useMemo(
    () =>
      groupPullRequests(
        filterPullRequests(pullRequests, {
          readiness: readiness === ALL ? null : readiness,
          repository: repository || null,
          author: author || null,
        })
      ),
    [pullRequests, readiness, repository, author]
  );

  const select = (pullRequest: PullRequest) => {
    const key = pullRequestKey(pullRequest);
    honoured.current = key;
    setSelectedKey(key);
    writeLink(key);
  };

  const close = () => {
    setSelectedKey(null);
    writeLink(null);
  };

  const merged = () => setMerges((count) => count + 1);

  const clearFilters = () => {
    setReadiness(ALL);
    setRepository(ANY);
    setAuthor(ANY);
  };

  const pullRequestIcon = <Icon path={PULL_REQUEST_PATH} />;

  const notices = (
    <SourceNotices
      className="pull-requests-notices"
      failure={listing.unavailable ? null : listing.error}
      unavailable={listing.unavailable}
      notices={noticesOf(listing, NOTICE_TEXTS)}
    />
  );

  const listPane = (content: ReactNode) => (
    <SubNav
      className="pull-requests-list-pane"
      storageKey={PANE_STORAGE_KEY}
      label="Resize pull request list"
      defaultWidthRem={PANE_WIDTH_REM}
    >
      {content}
    </SubNav>
  );

  const body = () => {
    if (!listed && listing.unavailable) {
      return (
        <EmptyState
          className="pull-requests-empty"
          icon={pullRequestIcon}
          title="GitHub is unavailable"
          description={UNAVAILABLE}
        />
      );
    }
    if (listing.loading) return listPane(<PullRequestsSkeleton />);
    if (!listed) {
      return (
        <div className="pull-requests-empty">
          <div className="error-banner" role="alert">
            {listing.error ?? LOAD_FAILED}
          </div>
        </div>
      );
    }
    if (sources.length === 0) {
      return (
        <NoSourcesState
          className="pull-requests-empty"
          icon={pullRequestIcon}
          description={NO_SOURCES_DESCRIPTION}
        />
      );
    }
    if (pullRequests.length === 0 && !selected) {
      return (
        <div className="pull-requests-empty">
          {notices}
          <EmptyState
            icon={pullRequestIcon}
            title="No open pull requests"
            description="Pull requests opened on this workspace's sources show up here."
          />
        </div>
      );
    }
    return (
      <>
        {listPane(
          <>
            {notices}
            <div className="pull-requests-list">
              {groups.length > 0 ? (
                <PullRequestList
                  groups={groups}
                  selected={selected ? selectedKey : null}
                  onSelect={select}
                />
              ) : (
                <EmptyState
                  className="pull-requests-filtered"
                  title="No matching pull requests"
                  description="Nothing open matches these filters"
                  action={
                    <Button variant="secondary" onClick={clearFilters}>
                      Clear filters
                    </Button>
                  }
                />
              )}
            </div>
          </>
        )}
        {selected ? (
          <PullRequestDetail
            key={selectedKey}
            pullRequest={selected}
            onClose={close}
            onMerged={merged}
          />
        ) : (
          <div className="pull-requests-detail-placeholder">
            <EmptyState
              icon={pullRequestIcon}
              title="Select a pull request"
              description="Choose one from the list to see its blockers, diff and commits."
            />
          </div>
        )}
      </>
    );
  };

  return (
    <div className="page page--workspace pull-requests-page">
      <PageBar
        title="Pull Requests"
        subtitle="Open pull requests and their readiness"
        titleRef={title}
      >
        <ReadinessFilter options={TABS} value={readiness} onChange={setReadiness} />
        <div className="pull-requests-filters">
          <Select
            compact
            value={repository}
            onValueChange={setRepository}
            aria-label="Filter by repository"
            options={filterOptions(repositories, 'All repositories')}
          />
          <Select
            compact
            value={author}
            onValueChange={setAuthor}
            aria-label="Filter by author"
            options={filterOptions(authors, 'All authors')}
          />
          <Button
            variant="ghost"
            size="icon"
            className="pull-requests-refresh"
            onClick={listing.refresh}
            disabled={!listing.refreshable && !listing.floored}
            aria-disabled={listing.floored || undefined}
            aria-describedby={listing.floored ? FLOORED_DESCRIPTION : undefined}
            aria-label={REFRESH_LABEL}
            aria-busy={listing.refreshing}
            title={listing.floored ? FLOORED : REFRESH_LABEL}
          >
            <Icon path={REFRESH_PATH} className="pull-request-icon" />
          </Button>
          {listing.floored && (
            <span id={FLOORED_DESCRIPTION} className="sr-only">
              {FLOORED}
            </span>
          )}
        </div>
      </PageBar>

      <div className="pull-requests-workspace">{body()}</div>
    </div>
  );
}

import { Button, EmptyState, Select } from '@zone/ui';
import { useMemo, useState } from 'react';
import { toast } from 'sonner';
import PageBar from '../../../shared/components/PageBar/PageBar';
import PermissionGate from '../../../shared/components/PermissionGate';
import PlusIcon from '../../../shared/components/PlusIcon/PlusIcon';
import { useWorkspace } from '../../../shared/context/WorkspaceContext';
import { PERMISSIONS } from '../../../shared/types/permissions';
import { Icon, NoSourcesState, noticesOf, REFRESH_PATH, SourceNotices } from '../../github';
import { CreateReleaseWizard, ReleaseDayGroup, ReleasesSkeleton, TAG_PATH } from '../components';
import { useReleases } from '../hooks/useReleases';
import type { Created } from '../types';
import { groupReleasesByDay } from '../utils/grouping';
import './ReleasesPage.css';

const ALL_REPOSITORIES = '';

export const REFRESH_LABEL = 'Refresh releases';
export const NEW_RELEASE = 'New release';
export const STALE_NOTICE = 'GitHub could not be reached, so these releases may be out of date.';
export const PARTIAL_NOTICE = 'Some repositories could not be read, so this history is incomplete.';
export const NO_SOURCES_DESCRIPTION = 'Connect a GitHub source to see its releases here';
export const NO_MATCHES_TITLE = 'No releases match';
export const NO_MATCHES_DESCRIPTION = 'This repository has no releases in this window';
export const SHOW_ALL = 'Show all repositories';

const NOTICE_TEXTS = { stale: STALE_NOTICE, partial: PARTIAL_NOTICE };

export function noReleasesDescription(days: number): string {
  return `Nothing was published in the last ${days} ${days === 1 ? 'day' : 'days'}`;
}

export function published(created: Created): string {
  return `Published ${created.tag}`;
}

export default function ReleasesPage() {
  const { currentWorkspace, writable } = useWorkspace();
  const workspaceId = currentWorkspace?.id;
  const [scope, setScope] = useState(workspaceId);
  const [repository, setRepository] = useState(ALL_REPOSITORIES);
  const [creating, setCreating] = useState(false);
  const { listing, releases, loading, error, unavailable, refresh, refreshing } = useReleases();

  if (scope !== workspaceId) {
    setScope(workspaceId);
    setRepository(ALL_REPOSITORIES);
    setCreating(false);
  }

  const sources = listing?.sources ?? [];
  const selected = sources.some((source) => source.source_id === repository)
    ? repository
    : ALL_REPOSITORIES;
  const filtered = selected !== ALL_REPOSITORIES;

  const days = useMemo(
    () =>
      groupReleasesByDay(
        filtered ? releases.filter((release) => release.source_id === selected) : releases
      ),
    [releases, filtered, selected]
  );

  const tagIcon = <Icon path={TAG_PATH} />;

  const renderBody = () => {
    if (loading) return <ReleasesSkeleton />;
    if (!listing) return null;
    if (days.length > 0) {
      return days.map((day) => <ReleaseDayGroup key={day.key} day={day} />);
    }
    if (sources.length === 0) {
      return (
        <NoSourcesState
          className="releases-empty"
          icon={tagIcon}
          description={NO_SOURCES_DESCRIPTION}
        />
      );
    }
    if (filtered) {
      return (
        <EmptyState
          className="releases-empty"
          icon={tagIcon}
          title={NO_MATCHES_TITLE}
          description={NO_MATCHES_DESCRIPTION}
          action={
            <Button variant="secondary" onClick={() => setRepository(ALL_REPOSITORIES)}>
              {SHOW_ALL}
            </Button>
          }
        />
      );
    }
    return (
      <EmptyState
        className="releases-empty"
        icon={tagIcon}
        title="No releases yet"
        description={noReleasesDescription(listing.window_days)}
      />
    );
  };

  return (
    <div className="page page--workspace releases-page">
      <PageBar title="Releases" subtitle="Recent releases and their pipelines">
        <div className="releases-filters">
          <Select
            compact
            value={selected}
            onValueChange={setRepository}
            aria-label="Filter by repository"
            disabled={loading || sources.length === 0}
            options={[
              { value: ALL_REPOSITORIES, label: 'All repositories' },
              ...sources.map((source) => ({
                value: source.source_id,
                label: source.repository,
              })),
            ]}
          />
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="releases-refresh"
          onClick={refresh}
          disabled={!workspaceId || refreshing}
          aria-label={REFRESH_LABEL}
          title={REFRESH_LABEL}
          data-refreshing={refreshing || undefined}
        >
          <Icon path={REFRESH_PATH} />
        </Button>
        <PermissionGate permission={PERMISSIONS.RELEASES.CREATE}>
          {writable && (
            <Button onClick={() => setCreating(true)} disabled={loading}>
              <PlusIcon />
              {NEW_RELEASE}
            </Button>
          )}
        </PermissionGate>
      </PageBar>

      <div className="page-body releases-body">
        <SourceNotices
          className="releases-notices"
          failure={unavailable ? null : error}
          unavailable={unavailable}
          notices={noticesOf(listing, NOTICE_TEXTS)}
        />
        {renderBody()}
      </div>

      <PermissionGate permission={PERMISSIONS.RELEASES.CREATE}>
        <CreateReleaseWizard
          isOpen={writable && creating}
          onClose={() => setCreating(false)}
          onCreated={(created) => toast.success(published(created))}
          sourceId={filtered ? selected : undefined}
        />
      </PermissionGate>
    </div>
  );
}

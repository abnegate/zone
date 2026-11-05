import { ActionMenu, type ActionMenuItem } from '@zone/ui';
import type { ChatPullRequest, PullRequestChange } from '../types';
import {
  mergeReading,
  pullRequestKind,
  pullRequestName,
  pullRequestTitle,
} from '../utils/pullRequests';
import { PullRequestStatusIcon } from './PullRequestStatusIcon';

export interface PullRequestItemProps {
  pullRequest: ChatPullRequest;
  busy: boolean;
  onChange: (pullRequest: ChatPullRequest, change: PullRequestChange) => void;
  onViewChanges: (pullRequest: ChatPullRequest) => void;
  onCopied: (pullRequest: ChatPullRequest) => void;
}

export const UNCREDENTIALED_HINT =
  'Needs write access and a connected GitHub source for this repository.';

function Icon({ path }: { path: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <path d={path} />
    </svg>
  );
}

const OPEN_ICON = 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5';
const COPY_ICON = 'M9 9h10v10H9zM5 15V5h10';
const CHANGES_ICON = 'M12 5v14M5 12h14M5 20h14';
const MERGE_ICON = 'M6 3v8a5 5 0 0 0 5 5h8M16 13l3 3-3 3';
const DRAFT_ICON = 'M4 20h4l10-10-4-4L4 16v4ZM13 7l4 4';
const DISMISS_ICON = 'M18 6 6 18M6 6l12 12';

function menuItems({
  pullRequest,
  onChange,
  onViewChanges,
  onCopied,
}: PullRequestItemProps): ActionMenuItem[] {
  const { status, actionable, url } = pullRequest;
  const uncredentialed = actionable ? undefined : UNCREDENTIALED_HINT;
  const items: ActionMenuItem[] = [
    {
      label: 'Open on GitHub',
      icon: <Icon path={OPEN_ICON} />,
      onSelect: () => window.open(url, '_blank', 'noopener,noreferrer'),
    },
    {
      label: 'Copy link',
      icon: <Icon path={COPY_ICON} />,
      onSelect: () => {
        void navigator.clipboard?.writeText(url).then(() => onCopied(pullRequest));
      },
    },
    {
      label: 'View changes',
      icon: <Icon path={CHANGES_ICON} />,
      disabled: !actionable,
      hint: uncredentialed,
      onSelect: () => onViewChanges(pullRequest),
    },
  ];
  if (status?.state === 'open') {
    const autoMerge = status.auto_merge;
    const mergeHint = uncredentialed ?? autoMerge.unavailable ?? undefined;
    const draftHint = uncredentialed ?? status.draft_unavailable ?? undefined;
    items.push(
      {
        label: autoMerge.enabled ? 'Cancel auto-merge' : 'Merge when ready',
        icon: <Icon path={MERGE_ICON} />,
        disabled: Boolean(mergeHint),
        hint: mergeHint,
        onSelect: () => onChange(pullRequest, { auto_merge: !autoMerge.enabled }),
      },
      {
        label: status.draft ? 'Ready for review' : 'Convert to draft',
        icon: <Icon path={DRAFT_ICON} />,
        disabled: Boolean(draftHint),
        hint: draftHint,
        onSelect: () => onChange(pullRequest, { draft: !status.draft }),
      }
    );
  }
  items.push({
    label: 'Dismiss',
    icon: <Icon path={DISMISS_ICON} />,
    destructive: true,
    onSelect: () => onChange(pullRequest, { dismissed: true }),
  });
  return items;
}

export function PullRequestItem(props: PullRequestItemProps) {
  const { pullRequest, busy } = props;
  const title = pullRequestTitle(pullRequest);
  const name = pullRequestName(pullRequest);
  const merge = mergeReading(pullRequest.status);

  return (
    <li className="chat-change" aria-busy={busy || undefined}>
      <PullRequestStatusIcon kind={pullRequestKind(pullRequest)} />
      <div className="chat-change-body">
        <a
          className="chat-change-title"
          href={pullRequest.url}
          target="_blank"
          rel="noopener noreferrer"
          title={title}
        >
          {title}
        </a>
        <div className="chat-change-meta">
          <span className="chat-change-name">{name}</span>
          {pullRequest.status && (
            <span className="chat-change-stat">
              <span className="chat-change-added">
                <span className="sr-only">added </span>+{pullRequest.status.additions}
              </span>
              <span className="chat-change-removed">
                <span className="sr-only">removed </span>
                {'−'}
                {pullRequest.status.deletions}
              </span>
            </span>
          )}
          {merge && (
            <span
              className={`chat-change-dot is-${merge.signal}`}
              role="img"
              aria-label={merge.label}
              title={merge.label}
            />
          )}
        </div>
      </div>
      <ActionMenu label={`Actions for ${name}`} items={menuItems(props)} />
    </li>
  );
}

import { ActionMenu, type ActionMenuItem } from '@zone/ui';
import type { ChatPullRequest, PullRequestChange } from '../types';
import {
  AUTO_MERGE_HINT,
  mergeReading,
  pullRequestKind,
  pullRequestName,
  pullRequestTitle,
  READ_ONLY_HINT,
  UNCREDENTIALED_HINT,
} from '../utils/pullRequests';
import { MenuIcon } from './MenuIcon';
import { PullRequestStatusIcon } from './PullRequestStatusIcon';
import { ReviewThreads } from './ReviewThreads';

export interface PullRequestItemProps {
  pullRequest: ChatPullRequest;
  busy: boolean;
  onChange: (pullRequest: ChatPullRequest, change: PullRequestChange) => void;
  onViewChanges: (pullRequest: ChatPullRequest) => void;
  onCopied: (pullRequest: ChatPullRequest, copied: boolean) => void;
}

const OPEN_ICON = 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5';
const COPY_ICON = 'M9 9h10v10H9zM5 15V5h10';
const CHANGES_ICON = 'M12 5v14M5 12h14M5 20h14';
const MERGE_ICON = 'M6 3v8a5 5 0 0 0 5 5h8M16 13l3 3-3 3';
const DRAFT_ICON = 'M4 20h4l10-10-4-4L4 16v4ZM13 7l4 4';
const FIX_ICON = 'M14.7 6.3a4 4 0 0 1-5 5L5 16v3h3l4.7-4.7a4 4 0 0 0 5-5l-2.4 2.4-2.1-2.1Z';
const DISMISS_ICON = 'M18 6 6 18M6 6l12 12';

function menuItems({
  pullRequest,
  onChange,
  onViewChanges,
  onCopied,
}: PullRequestItemProps): ActionMenuItem[] {
  const { status, writable, credentialed, url } = pullRequest;
  const readOnly = writable ? undefined : READ_ONLY_HINT;
  const uncredentialed = credentialed ? undefined : UNCREDENTIALED_HINT;
  const items: ActionMenuItem[] = [
    {
      label: 'Open on GitHub',
      icon: <MenuIcon path={OPEN_ICON} />,
      onSelect: () => window.open(url, '_blank', 'noopener,noreferrer'),
    },
    {
      label: 'Copy link',
      icon: <MenuIcon path={COPY_ICON} />,
      onSelect: () => {
        void navigator.clipboard
          ?.writeText(url)
          .then(() => onCopied(pullRequest, true))
          .catch(() => onCopied(pullRequest, false));
      },
    },
    {
      label: 'View changes',
      icon: <MenuIcon path={CHANGES_ICON} />,
      disabled: !credentialed,
      hint: uncredentialed,
      onSelect: () => onViewChanges(pullRequest),
    },
  ];
  const open = status?.state === 'open';
  if (open) {
    const autoMerge = status.auto_merge;
    const mergeHint = readOnly ?? uncredentialed ?? autoMerge.unavailable ?? undefined;
    const draftHint = readOnly ?? uncredentialed ?? status.draft_unavailable ?? undefined;
    items.push(
      {
        label: autoMerge.enabled ? 'Cancel auto-merge' : 'Merge when ready',
        icon: <MenuIcon path={MERGE_ICON} />,
        disabled: Boolean(mergeHint),
        hint: mergeHint,
        onSelect: () => onChange(pullRequest, { auto_merge: !autoMerge.enabled }),
      },
      {
        label: status.draft ? 'Ready for review' : 'Convert to draft',
        icon: <MenuIcon path={DRAFT_ICON} />,
        disabled: Boolean(draftHint),
        hint: draftHint,
        onSelect: () => onChange(pullRequest, { draft: !status.draft }),
      }
    );
  }
  if (pullRequest.auto_fix) {
    items.push({
      label: 'Stop fixing automatically',
      icon: <MenuIcon path={FIX_ICON} />,
      disabled: !writable,
      hint: readOnly,
      onSelect: () => onChange(pullRequest, { auto_fix: false }),
    });
  } else if (open) {
    const fixHint = readOnly ?? uncredentialed;
    items.push({
      label: 'Fix automatically',
      icon: <MenuIcon path={FIX_ICON} />,
      disabled: Boolean(fixHint),
      hint: fixHint,
      onSelect: () => onChange(pullRequest, { auto_fix: true }),
    });
  }
  items.push({
    label: 'Dismiss',
    icon: <MenuIcon path={DISMISS_ICON} />,
    destructive: true,
    disabled: !writable,
    hint: readOnly,
    onSelect: () => onChange(pullRequest, { dismissed: true }),
  });
  return items;
}

export function PullRequestItem(props: PullRequestItemProps) {
  const { pullRequest, busy } = props;
  const title = pullRequestTitle(pullRequest);
  const name = pullRequestName(pullRequest);
  const merge = mergeReading(pullRequest.status);
  const autoMerging = pullRequest.status?.state === 'open' && pullRequest.status.auto_merge.enabled;

  return (
    <li className="chat-change" data-pull-request={pullRequest.id} aria-busy={busy || undefined}>
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
          {pullRequest.status?.threads && <ReviewThreads threads={pullRequest.status.threads} />}
          {autoMerging && (
            <span className="chat-change-auto" title={AUTO_MERGE_HINT}>
              Auto-merge
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

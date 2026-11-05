import { type ReactElement, useEffect, useRef, useState } from 'react';
import type { ChatPullRequests } from '../hooks/useChatPullRequests';
import type { ChatPullRequest } from '../types';
import { panelKey } from '../utils/panel';
import { pullRequestName } from '../utils/pullRequests';
import { DiffModal } from './DiffModal';
import { PanelBox } from './PanelBox';
import { PullRequestItem } from './PullRequestItem';

export interface ChangesBoxProps {
  chatId: string;
  userId: string | null;
  changes: ChatPullRequests;
}

export function ChangesBox({ chatId, userId, changes }: ChangesBoxProps): ReactElement {
  const { pullRequests, dismissed, pending, error, change, undo } = changes;
  const [viewing, setViewing] = useState<ChatPullRequest | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const list = useRef<HTMLUListElement>(null);
  const undoButton = useRef<HTMLButtonElement>(null);
  const restoring = useRef<string | null>(null);
  const dismissedId = dismissed?.id ?? null;

  useEffect(() => {
    if (dismissedId) undoButton.current?.focus();
  }, [dismissedId]);

  useEffect(() => {
    const id = restoring.current;
    if (!id || !pullRequests.some((pullRequest) => pullRequest.id === id)) return;
    restoring.current = null;
    const rows = list.current?.querySelectorAll<HTMLElement>('[data-pull-request]') ?? [];
    Array.from(rows)
      .find((row) => row.dataset.pullRequest === id)
      ?.querySelector<HTMLElement>('.chat-change-title')
      ?.focus();
  }, [pullRequests]);

  return (
    <PanelBox
      title="Changes"
      count={pullRequests.length}
      storageKey={panelKey(userId, 'changes')}
      className="chat-changes"
    >
      {pullRequests.length > 0 && (
        <ul ref={list} className="chat-change-list">
          {pullRequests.map((pullRequest) => (
            <PullRequestItem
              key={pullRequest.id}
              pullRequest={pullRequest}
              busy={pending.has(pullRequest.id)}
              onChange={(target, requested) => {
                setNotice(null);
                void change(target, requested);
              }}
              onViewChanges={setViewing}
              onCopied={(target, copied) =>
                setNotice(
                  copied
                    ? `Copied the link to ${pullRequestName(target)}.`
                    : `The link to ${pullRequestName(target)} could not be copied.`
                )
              }
            />
          ))}
        </ul>
      )}
      <div className="chat-changes-status" aria-live="polite">
        {dismissed && (
          <p className="chat-changes-notice">
            Dismissed {pullRequestName(dismissed)}.{' '}
            <button
              ref={undoButton}
              type="button"
              className="chat-changes-undo"
              onClick={() => {
                restoring.current = dismissed.id;
                void undo();
              }}
            >
              Undo
            </button>
          </p>
        )}
        {notice && <p className="chat-changes-notice">{notice}</p>}
      </div>
      {error && (
        <p className="chat-changes-error" role="alert">
          {error}
        </p>
      )}
      <DiffModal chatId={chatId} pullRequest={viewing} onClose={() => setViewing(null)} />
    </PanelBox>
  );
}

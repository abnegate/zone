import { useState } from 'react';
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

/** The pull requests this chat opened, newest last, with what can be done to each. */
export function ChangesBox({ chatId, userId, changes }: ChangesBoxProps) {
  const { pullRequests, dismissed, pending, error, change, undo } = changes;
  const [viewing, setViewing] = useState<ChatPullRequest | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  return (
    <PanelBox
      title="Changes"
      count={pullRequests.length}
      storageKey={panelKey(userId, 'changes')}
      className="chat-changes"
    >
      {pullRequests.length > 0 && (
        <ul className="chat-change-list">
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
              onCopied={(target) => setNotice(`Copied the link to ${pullRequestName(target)}.`)}
            />
          ))}
        </ul>
      )}
      <div className="chat-changes-status" aria-live="polite">
        {dismissed && (
          <p className="chat-changes-notice">
            Dismissed {pullRequestName(dismissed)}.{' '}
            <button type="button" className="chat-changes-undo" onClick={() => void undo()}>
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

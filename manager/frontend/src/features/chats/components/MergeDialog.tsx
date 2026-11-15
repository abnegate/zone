import { Button, Modal } from '@zone/ui';
import type { ChatPullRequest } from '../types';
import { pullRequestName, pullRequestTitle } from '../utils/pullRequests';

export interface MergeDialogProps {
  pullRequest: ChatPullRequest | null;
  busy: boolean;
  onConfirm: (pullRequest: ChatPullRequest) => void;
  onClose: () => void;
}

/**
 * The step between the menu and the merge. A merge lands on the base branch
 * and nothing in Zone takes it back, so the reader says which pull request
 * they meant before GitHub is asked.
 */
export function MergeDialog({ pullRequest, busy, onConfirm, onClose }: MergeDialogProps) {
  return (
    <Modal
      isOpen={pullRequest !== null}
      onClose={onClose}
      title={pullRequest ? `Merge ${pullRequestName(pullRequest)}?` : 'Merge'}
      size="sm"
      className="chat-merge-modal"
    >
      {pullRequest && (
        <div className="chat-merge-confirm">
          <p className="chat-merge-subject">{pullRequestTitle(pullRequest)}</p>
          <p className="chat-merge-warning">
            It merges on GitHub by the first method the repository allows, at the commit its status
            was read at. Nothing here undoes that.
          </p>
          <div className="chat-merge-actions">
            <Button type="button" variant="outline" size="sm" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button
              type="button"
              size="sm"
              loading={busy}
              disabled={busy}
              onClick={() => onConfirm(pullRequest)}
            >
              Merge
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}

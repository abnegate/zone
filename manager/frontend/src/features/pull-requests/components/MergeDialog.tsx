import { Button, Modal, Select } from '@zone/ui';
import { useRef, useState } from 'react';
import { toast } from 'sonner';
import { shortSha } from '../../github';
import { useMergePullRequest } from '../hooks';
import { MergeMethodSchema, RefusalCodeSchema } from '../schemas';
import type { MergeMethod, PullRequest } from '../types';

export const HEAD_MOVED = 'New commits were pushed. Review them before merging.';

export const OPEN_ON_GITHUB = 'Open the pull request on GitHub';

const METHOD_LABELS: Readonly<Record<MergeMethod, string>> = {
  merge: 'Create a merge commit',
  squash: 'Squash and merge',
  rebase: 'Rebase and merge',
};

const METHOD_OPTIONS = MergeMethodSchema.options.map((method) => ({
  value: method,
  label: METHOD_LABELS[method],
}));

function isMergeMethod(value: string): value is MergeMethod {
  return MergeMethodSchema.safeParse(value).success;
}

export interface MergeDialogProps {
  isOpen: boolean;
  pullRequest: PullRequest;
  onClose: () => void;
  onMerged?: () => void;
}

export function MergeDialog({ isOpen, pullRequest, onClose, onMerged }: MergeDialogProps) {
  const { merge, merging, error, refusal, settled, unknown, reset } = useMergePullRequest();
  const [method, setMethod] = useState<MergeMethod>(MergeMethodSchema.enum.merge);
  const [reviewed] = useState(pullRequest.head);
  const submitting = useRef(false);
  const moved =
    pullRequest.head !== reviewed || refusal?.code === RefusalCodeSchema.enum.head_moved;
  const held = moved || settled || unknown;

  const close = () => {
    if (merging) return;
    reset();
    onClose();
  };

  const submit = async () => {
    if (submitting.current || held) return;
    submitting.current = true;
    try {
      const merged = await merge({ ...pullRequest, head: reviewed }, method);
      toast.success(`Merged ${pullRequest.repository} #${merged.number}`);
      reset();
      onClose();
      onMerged?.();
    } catch {
      return;
    } finally {
      submitting.current = false;
    }
  };

  return (
    <Modal isOpen={isOpen} onClose={close} title="Merge pull request" size="sm">
      <p className="merge-dialog-summary">
        Merge <strong>#{pullRequest.number}</strong> {pullRequest.title} from{' '}
        <code>{pullRequest.head_ref}</code> into <code>{pullRequest.base_ref}</code>
        {reviewed && <> at {shortSha(reviewed)}</>}.
      </p>
      <Select
        label="Merge method"
        value={method}
        onValueChange={(value) => isMergeMethod(value) && setMethod(value)}
        options={METHOD_OPTIONS}
        disabled={merging}
      />
      {moved && (
        <p className="source-notice" role="status">
          {HEAD_MOVED}
        </p>
      )}
      {error && (
        <div className="form-error" role="alert">
          {error}
          {refusal && refusal.blockers.length > 0 && (
            <ul className="merge-dialog-blockers">
              {refusal.blockers.map((blocker) => (
                <li key={`${blocker.code}:${blocker.detail}`}>{blocker.detail}</li>
              ))}
            </ul>
          )}
          {unknown && (
            <a
              className="merge-dialog-link"
              href={pullRequest.url}
              target="_blank"
              rel="noopener noreferrer"
            >
              {OPEN_ON_GITHUB}
            </a>
          )}
        </div>
      )}
      <div className="modal-actions">
        <Button variant="secondary" type="button" onClick={close} disabled={merging}>
          Cancel
        </Button>
        <Button type="button" onClick={submit} disabled={merging || held} loading={merging}>
          {merging ? 'Merging...' : 'Merge'}
        </Button>
      </div>
    </Modal>
  );
}

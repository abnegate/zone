import { Modal } from '@zone/ui';
import { useEffect, useState } from 'react';
import { chatsApi } from '../../../api/chats';
import type { ChatPullRequest } from '../types';
import { type DiffFile, splitDiff } from '../utils/diff';
import { pullRequestName } from '../utils/pullRequests';

export interface DiffModalProps {
  chatId: string;
  pullRequest: ChatPullRequest | null;
  onClose: () => void;
}

interface Loaded {
  id: string;
  files: DiffFile[];
  truncated: boolean;
}

/** The files a pull request changes, each as its own unified diff. */
export function DiffModal({ chatId, pullRequest, onClose }: DiffModalProps) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const id = pullRequest?.id ?? null;

  useEffect(() => {
    if (!id) return;
    let current = true;
    setLoaded(null);
    setError(null);
    chatsApi
      .getChatPullRequestDiff(chatId, id)
      .then((diff) => {
        if (current) setLoaded({ id, files: splitDiff(diff.diff), truncated: diff.truncated });
      })
      .catch((failure: unknown) => {
        if (current) {
          setError(failure instanceof Error ? failure.message : 'Failed to load the changes');
        }
      });
    return () => {
      current = false;
    };
  }, [chatId, id]);

  const shown = loaded && loaded.id === id ? loaded : null;

  return (
    <Modal
      isOpen={pullRequest !== null}
      onClose={onClose}
      title={pullRequest ? `Changes in ${pullRequestName(pullRequest)}` : 'Changes'}
      size="xl"
      className="chat-diff-modal"
    >
      {error ? (
        <p className="chat-diff-state" role="alert">
          {error}
        </p>
      ) : !shown ? (
        <p className="chat-diff-state" role="status">
          Loading changes…
        </p>
      ) : shown.files.length === 0 ? (
        <p className="chat-diff-state">No file changes to show.</p>
      ) : (
        <div className="chat-diff">
          {shown.files.map((file) => (
            <details key={file.path} className="chat-diff-file" open>
              <summary className="chat-diff-file-name">
                <span className="chat-diff-path">{file.path}</span>
                <span className="chat-change-added">+{file.additions}</span>
                <span className="chat-change-removed">
                  {'−'}
                  {file.deletions}
                </span>
              </summary>
              <pre className="chat-diff-lines">
                {file.lines.map((line, index) => (
                  <span
                    // biome-ignore lint/suspicious/noArrayIndexKey: a diff line is identified only by where it sits
                    key={index}
                    className={`chat-diff-line is-${line.kind}`}
                  >
                    {line.text}
                    {'\n'}
                  </span>
                ))}
              </pre>
            </details>
          ))}
          {shown.truncated && (
            <p className="chat-diff-state">
              The diff is too large to show in full. Open it on GitHub for the rest.
            </p>
          )}
        </div>
      )}
    </Modal>
  );
}

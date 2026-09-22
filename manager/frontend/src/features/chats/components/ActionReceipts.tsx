import { Link } from 'react-router-dom';
import { type ActionReceipt, type ActionTarget, REASON_LABEL } from '../types';
import { formatDate } from '../utils';
import './ActionReceipts.css';

const ACTION_LABELS: Record<string, { ok: string; failed: string }> = {
  create_task: { ok: 'Created task', failed: 'Create task failed' },
  update_task: { ok: 'Updated task', failed: 'Update task failed' },
  create_document: { ok: 'Created document', failed: 'Create document failed' },
  update_document: { ok: 'Updated document', failed: 'Update document failed' },
  send_message: { ok: 'Sent message', failed: 'Send message failed' },
  create_reminder: { ok: 'Created reminder', failed: 'Create reminder failed' },
  cancel_reminder: { ok: 'Cancelled reminder', failed: 'Cancel reminder failed' },
  memory_write: { ok: 'Wrote memory', failed: 'Write memory failed' },
  memory_append: { ok: 'Appended to memory', failed: 'Append to memory failed' },
  memory_delete: { ok: 'Forgot memory', failed: 'Forget memory failed' },
  finalize_project: { ok: 'Created project', failed: 'Create project failed' },
};

const LINK_LABELS: Record<ActionTarget, string> = {
  task: 'Open task',
  document: 'Open document',
  message: 'Open message',
  reminder: 'Open chat',
  memory: 'Open memory',
  project: 'Open project',
};

function actionLabel(receipt: ActionReceipt): string {
  const labels = ACTION_LABELS[receipt.action];
  if (!labels) {
    return receipt.success ? receipt.action : `${receipt.action} failed`;
  }
  return receipt.success ? labels.ok : labels.failed;
}

function ReceiptCard({ receipt }: { receipt: ActionReceipt }) {
  const status = receipt.success ? 'ok' : 'failed';
  const stated = receipt.reason?.trim();

  return (
    <article className={`action-receipt action-receipt--${status}`} data-testid="action-receipt">
      <header className="action-receipt-header">
        <span className="action-receipt-status" aria-hidden="true" />
        <h3 className="action-receipt-action">{actionLabel(receipt)}</h3>
        <time className="action-receipt-time" dateTime={receipt.occurred_at}>
          {formatDate(receipt.occurred_at)}
        </time>
      </header>
      <p className="action-receipt-target">{receipt.target_label}</p>
      {stated ? (
        <p className="action-receipt-reason" data-testid="action-receipt-reason">
          <span className="action-receipt-reason-label">{REASON_LABEL} </span>
          {stated}
        </p>
      ) : null}
      <footer className="action-receipt-meta">
        <span>{receipt.actor_name || receipt.actor_id}</span>
        <span>{receipt.outcome}</span>
        {receipt.href ? (
          <Link to={receipt.href} className="action-receipt-link" data-testid="action-receipt-link">
            {LINK_LABELS[receipt.target_type]}
          </Link>
        ) : null}
      </footer>
    </article>
  );
}

export function ActionReceipts({ receipts }: { receipts: ActionReceipt[] }) {
  if (receipts.length === 0) return null;

  return (
    <div className="action-receipts" data-testid="action-receipts">
      {receipts.map((receipt) => (
        <ReceiptCard key={receipt.id} receipt={receipt} />
      ))}
    </div>
  );
}

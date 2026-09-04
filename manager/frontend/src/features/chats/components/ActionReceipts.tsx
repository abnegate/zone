import { Link } from 'react-router-dom';
import type { ActionReceipt, ActionTarget } from '../types';
import { formatDate } from '../utils';

const ACTION_LABELS: Record<string, { ok: string; failed: string }> = {
  create_task: { ok: 'Created task', failed: 'Create task failed' },
  update_task: { ok: 'Updated task', failed: 'Update task failed' },
  create_document: { ok: 'Created document', failed: 'Create document failed' },
  update_document: { ok: 'Updated document', failed: 'Update document failed' },
  send_message: { ok: 'Sent message', failed: 'Send message failed' },
  create_reminder: { ok: 'Created reminder', failed: 'Create reminder failed' },
  cancel_reminder: { ok: 'Cancelled reminder', failed: 'Cancel reminder failed' },
};

const LINK_LABELS: Record<ActionTarget, string> = {
  task: 'Open task',
  document: 'Open document',
  message: 'Open message',
  reminder: 'Open chat',
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

  return (
    <article className={`action-receipt action-receipt--${status}`} data-testid="action-receipt">
      <header className="action-receipt-header">
        <span className="action-receipt-status" aria-hidden="true" />
        <h3 className="action-receipt-action">{actionLabel(receipt)}</h3>
      </header>
      <p className="action-receipt-target">{receipt.target_label}</p>
      <dl className="action-receipt-meta">
        <div>
          <dt>Actor</dt>
          <dd>{receipt.actor_name || receipt.actor_id}</dd>
        </div>
        <div>
          <dt>When</dt>
          <dd>
            <time dateTime={receipt.occurred_at}>{formatDate(receipt.occurred_at)}</time>
          </dd>
        </div>
        <div>
          <dt>Outcome</dt>
          <dd>{receipt.outcome}</dd>
        </div>
      </dl>
      {receipt.href ? (
        <Link to={receipt.href} className="action-receipt-link" data-testid="action-receipt-link">
          {LINK_LABELS[receipt.target_type]}
        </Link>
      ) : null}
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

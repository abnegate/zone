import type { ReactElement } from 'react';
import { type Transfer, TransferStatus } from '../../hooks/useTransfers';
import './TransferChip.css';

type TransferChipProps = {
  transfer: Transfer;
  processing?: string;
  onCancel: (id: string) => void;
  onRetry: (id: string) => void;
};

function describe(transfer: Transfer, percent: number, processing: string): string {
  switch (transfer.status) {
    case TransferStatus.Queued:
      return 'Waiting';
    case TransferStatus.Uploading:
      return `${percent}%`;
    case TransferStatus.Processing:
      return processing;
    case TransferStatus.Failed:
      return transfer.error ?? 'The upload failed';
  }
}

export default function TransferChip({
  transfer,
  processing = 'Processing…',
  onCancel,
  onRetry,
}: TransferChipProps): ReactElement {
  const failed = transfer.status === TransferStatus.Failed;
  const percent = Math.round(transfer.progress * 100);
  const status = describe(transfer, percent, processing);

  return (
    <div
      className={`transfer-chip transfer-chip--${transfer.status}`}
      role="group"
      aria-label={`${transfer.name}: ${status}`}
      data-testid="transfer-chip"
    >
      {failed ? (
        <span className="transfer-chip-alert" aria-hidden="true">
          !
        </span>
      ) : (
        <span className="spinner transfer-chip-spinner" aria-hidden="true" />
      )}
      <span className="transfer-chip-body">
        <span className="transfer-chip-line">
          <span className="transfer-chip-name">{transfer.name}</span>
          <span
            className="transfer-chip-status"
            role={failed ? 'alert' : undefined}
            title={failed ? status : undefined}
          >
            {status}
          </span>
        </span>
        {failed ? null : (
          <span
            className="transfer-chip-bar"
            role="progressbar"
            aria-label={`Uploading ${transfer.name}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
          >
            <span className="transfer-chip-fill" style={{ width: `${percent}%` }} />
          </span>
        )}
      </span>
      {failed ? (
        <button
          type="button"
          className="transfer-chip-action"
          onClick={() => onRetry(transfer.id)}
          aria-label={`Retry ${transfer.name}`}
        >
          Retry
        </button>
      ) : null}
      <button
        type="button"
        className="transfer-chip-action transfer-chip-dismiss"
        onClick={() => onCancel(transfer.id)}
        aria-label={`${failed ? 'Remove' : 'Cancel'} ${transfer.name}`}
      >
        ×
      </button>
    </div>
  );
}

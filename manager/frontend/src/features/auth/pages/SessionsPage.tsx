import {
  createColumnHelper,
  flexRender,
  type StockFeatures,
  stockFeatures,
  useTable,
} from '@tanstack/react-table';
import { Button, Modal } from '@zone/ui';
import { formatDistanceToNow } from 'date-fns';
import { useState } from 'react';
import { toast } from 'sonner';
import { useSessions } from '../hooks';
import type { Session } from '../types';
import { parseUserAgent } from '../utils';
import './SessionsPage.css';

const columnHelper = createColumnHelper<StockFeatures, Session>();

export default function SessionsPage() {
  const {
    sessions,
    isLoading,
    error,
    revokeSession,
    isRevoking,
    revokeAllSessions,
    isRevokingAll,
  } = useSessions();

  const [sessionToRevoke, setSessionToRevoke] = useState<string | null>(null);
  const [showRevokeModal, setShowRevokeModal] = useState(false);
  const [showRevokeAllModal, setShowRevokeAllModal] = useState(false);

  const columns = columnHelper.columns([
    columnHelper.accessor((row) => row.device_info || parseUserAgent(row.user_agent), {
      id: 'device',
      header: 'Device / Browser',
      cell: (info) => (
        <div className="device-info">
          {info.getValue()}
          {info.row.original.is_current && <span className="current-badge">Current Session</span>}
        </div>
      ),
    }),
    columnHelper.accessor('location', {
      header: 'Location',
      cell: (info) => info.getValue() || 'Unknown',
    }),
    columnHelper.accessor('ip_address', {
      header: 'IP Address',
      cell: (info) => <span className="ip-address">{info.getValue() || 'Unknown'}</span>,
    }),
    columnHelper.accessor('last_active_at', {
      header: 'Last Active',
      cell: (info) => (
        <span className="timestamp">
          {formatDistanceToNow(new Date(info.getValue()), { addSuffix: true })}
        </span>
      ),
    }),
    columnHelper.accessor('created_at', {
      header: 'Created',
      cell: (info) => (
        <span className="timestamp">
          {formatDistanceToNow(new Date(info.getValue()), { addSuffix: true })}
        </span>
      ),
    }),
    columnHelper.display({
      id: 'actions',
      header: 'Actions',
      cell: (info) => (
        <Button
          onClick={() => {
            setSessionToRevoke(info.row.original.id);
            setShowRevokeModal(true);
          }}
          disabled={info.row.original.is_current || isRevoking}
          variant="secondary"
          size="sm"
          aria-label={
            info.row.original.is_current
              ? 'Cannot revoke current session'
              : `Revoke session from ${
                  info.row.original.device_info || parseUserAgent(info.row.original.user_agent)
                }`
          }
        >
          Revoke
        </Button>
      ),
    }),
  ]);

  const table = useTable({
    features: stockFeatures,
    data: sessions,
    columns,
  });

  const handleRevokeConfirm = async () => {
    if (!sessionToRevoke) return;
    try {
      await revokeSession(sessionToRevoke);
      toast.success('Session revoked successfully');
      setShowRevokeModal(false);
      setSessionToRevoke(null);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to revoke session');
    }
  };

  const handleRevokeAllConfirm = async () => {
    try {
      await revokeAllSessions();
      toast.success('All other sessions revoked successfully');
      setShowRevokeAllModal(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to revoke all sessions');
    }
  };

  const nonCurrentSessions = sessions.filter((s) => !s.is_current);
  const hasOtherSessions = nonCurrentSessions.length > 0;

  if (isLoading) {
    return (
      <div className="sessions-page page--workspace">
        <div className="page-header">
          <h1 className="page-title">Active Sessions</h1>
        </div>
        <div className="loading-state">Loading sessions...</div>
      </div>
    );
  }

  return (
    <div className="sessions-page page--workspace">
      <div className="page-header">
        <h1 className="page-title">Active Sessions</h1>
        <p className="page-description">
          Manage your active sessions across all devices. You can revoke access to any session at
          any time.
        </p>
      </div>

      {error && (
        <div className="alert alert-error" role="alert" aria-live="assertive">
          {error}
        </div>
      )}

      <div className="sessions-container">
        {sessions.length === 0 ? (
          <div className="empty-state">
            <p>No active sessions found.</p>
          </div>
        ) : (
          <>
            <div className="sessions-table-wrapper">
              <table className="sessions-table" aria-label="Active sessions">
                <thead>
                  {table.getHeaderGroups().map((headerGroup) => (
                    <tr key={headerGroup.id}>
                      {headerGroup.headers.map((header) => (
                        <th key={header.id}>
                          {header.isPlaceholder
                            ? null
                            : flexRender(header.column.columnDef.header, header.getContext())}
                        </th>
                      ))}
                    </tr>
                  ))}
                </thead>
                <tbody>
                  {table.getRowModel().rows.map((row) => (
                    <tr key={row.id} className={row.original.is_current ? 'current-session' : ''}>
                      {row.getVisibleCells().map((cell) => (
                        <td key={cell.id}>
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <div className="sessions-actions">
              <Button
                onClick={() => setShowRevokeAllModal(true)}
                disabled={!hasOtherSessions || isRevokingAll}
                variant="danger"
              >
                Revoke All Other Sessions
              </Button>
            </div>
          </>
        )}
      </div>

      {/* Revoke Single Session Modal */}
      <Modal
        isOpen={showRevokeModal}
        onClose={() => {
          if (!isRevoking) {
            setShowRevokeModal(false);
            setSessionToRevoke(null);
          }
        }}
        title="Revoke Session"
      >
        <p>Are you sure you want to revoke this session? This action cannot be undone.</p>
        <div className="modal-actions">
          <Button
            onClick={() => {
              setShowRevokeModal(false);
              setSessionToRevoke(null);
            }}
            disabled={isRevoking}
            variant="secondary"
          >
            Cancel
          </Button>
          <Button onClick={handleRevokeConfirm} loading={isRevoking} variant="danger">
            Confirm
          </Button>
        </div>
      </Modal>

      {/* Revoke All Sessions Modal */}
      <Modal
        isOpen={showRevokeAllModal}
        onClose={() => {
          if (!isRevokingAll) {
            setShowRevokeAllModal(false);
          }
        }}
        title="Revoke All Other Sessions"
      >
        <p>
          Are you sure you want to revoke all other sessions? This will sign you out from all
          devices except this one. This action cannot be undone.
        </p>
        <div className="modal-actions">
          <Button
            onClick={() => setShowRevokeAllModal(false)}
            disabled={isRevokingAll}
            variant="secondary"
          >
            Cancel
          </Button>
          <Button onClick={handleRevokeAllConfirm} loading={isRevokingAll} variant="danger">
            Confirm
          </Button>
        </div>
      </Modal>
    </div>
  );
}

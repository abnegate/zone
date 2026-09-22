import { Button, EmptyState } from '@zone/ui';
import { Fragment, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import { AUDIT_ACTIONS, AUDIT_RESOURCE_TYPES } from '../schemas';
import type { AuditAction, AuditLog, AuditLogFilters, AuditResourceType } from '../types';
import './AuditLogsSection.css';

interface AuditLogsSectionProps {
  orgId: string;
}

const ACTIONS: readonly AuditAction[] = AUDIT_ACTIONS;
const RESOURCE_TYPES: readonly AuditResourceType[] = AUDIT_RESOURCE_TYPES;

export function AuditLogsSection({ orgId }: AuditLogsSectionProps) {
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [logs, setLogs] = useState<AuditLog[]>([]);
  const [total, setTotal] = useState(0);
  const [expandedLog, setExpandedLog] = useState<string | null>(null);

  // Filters
  const [action, setAction] = useState<AuditAction | ''>('');
  const [resourceType, setResourceType] = useState<AuditResourceType | ''>('');
  const [actorFilter, setActorFilter] = useState('');
  const [startDate, setStartDate] = useState('');
  const [endDate, setEndDate] = useState('');
  const [showFilters, setShowFilters] = useState(false);

  // Pagination
  const [offset, setOffset] = useState(0);
  const limit = 50;

  const loadLogs = useCallback(async () => {
    if (!orgId) return;
    setLoading(true);
    setError(null);

    try {
      const filters: AuditLogFilters = {
        limit,
        offset,
      };

      if (action) filters.action = action;
      if (resourceType) filters.resource_type = resourceType;
      if (actorFilter) filters.actor_id = actorFilter;
      if (startDate) filters.start_date = startDate;
      if (endDate) filters.end_date = endDate;

      const response = await client.getAuditLogs(orgId, filters);
      setLogs(response.logs);
      setTotal(response.total);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load audit logs');
    } finally {
      setLoading(false);
    }
  }, [orgId, action, resourceType, actorFilter, startDate, endDate, offset]);

  useEffect(() => {
    loadLogs();
  }, [loadLogs]);

  const handleApplyFilters = () => {
    setOffset(0); // Reset to first page
    loadLogs();
  };

  const handleResetFilters = () => {
    setAction('');
    setResourceType('');
    setActorFilter('');
    setStartDate('');
    setEndDate('');
    setOffset(0);
  };

  const handleExport = async () => {
    setExporting(true);
    setError(null);

    try {
      const filters: AuditLogFilters = {};
      if (action) filters.action = action;
      if (resourceType) filters.resource_type = resourceType;
      if (actorFilter) filters.actor_id = actorFilter;
      if (startDate) filters.start_date = startDate;
      if (endDate) filters.end_date = endDate;

      const blob = await client.exportAuditLogs(orgId, filters);

      // Create download link
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `audit-logs-${new Date().toISOString().split('T')[0]}.csv`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      window.URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export audit logs');
    } finally {
      setExporting(false);
    }
  };

  const handleLoadMore = () => {
    setOffset(offset + limit);
  };

  const toggleExpanded = (logId: string) => {
    setExpandedLog(expandedLog === logId ? null : logId);
  };

  const formatTimestamp = (timestamp: string): { relative: string; absolute: string } => {
    const date = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    let relative = '';
    if (diffMins < 1) {
      relative = 'just now';
    } else if (diffMins < 60) {
      relative = `${diffMins}m ago`;
    } else if (diffHours < 24) {
      relative = `${diffHours}h ago`;
    } else if (diffDays < 7) {
      relative = `${diffDays}d ago`;
    } else {
      relative = date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    }

    const absolute = date.toLocaleString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    return { relative, absolute };
  };

  const getActionBadgeClass = (action: AuditAction): string => {
    const outcome = action.split('.').pop() ?? '';
    switch (outcome) {
      case 'created':
      case 'added':
      case 'sent':
        return 'action-create';
      case 'updated':
      case 'role_changed':
        return 'action-update';
      case 'deleted':
      case 'removed':
        return 'action-delete';
      case 'accepted':
        return 'action-success';
      case 'revoked':
      case 'reset':
        return 'action-warning';
      default:
        return 'action-default';
    }
  };

  if (loading && offset === 0) {
    return <div className="loading-state">Loading audit logs...</div>;
  }
  if (error && offset === 0) {
    return (
      <div className="alert alert-error alert-inline" role="alert">
        <span>{error}</span>
        <Button onClick={loadLogs} variant="ghost" size="sm">
          Retry
        </Button>
      </div>
    );
  }

  const hasMore = offset + logs.length < total;
  const hasActiveFilters = action || resourceType || actorFilter || startDate || endDate;

  return (
    <div className="audit-logs-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Audit Logs</h2>
          <p className="section-description">
            {total} total {total === 1 ? 'entry' : 'entries'} for this organization.
          </p>
        </div>
        <div className="section-row-actions">
          <Button onClick={() => setShowFilters(!showFilters)} variant="ghost" size="sm">
            {showFilters ? 'Hide Filters' : 'Show Filters'}
          </Button>
          <Button onClick={handleExport} loading={exporting} size="sm">
            Export CSV
          </Button>
        </div>
      </div>

      {showFilters && (
        <div className="audit-logs-filters">
          <div className="filter-grid">
            <div className="form-group">
              <label htmlFor="action-filter">Action</label>
              <select
                id="action-filter"
                value={action}
                onChange={(e) => setAction(e.target.value as AuditAction | '')}
                className="form-select"
              >
                <option value="">All Actions</option>
                {ACTIONS.map((a) => (
                  <option key={a} value={a}>
                    {a}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label htmlFor="resource-type-filter">Resource Type</label>
              <select
                id="resource-type-filter"
                value={resourceType}
                onChange={(e) => setResourceType(e.target.value as AuditResourceType | '')}
                className="form-select"
              >
                <option value="">All Types</option>
                {RESOURCE_TYPES.map((type) => (
                  <option key={type} value={type}>
                    {type}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label htmlFor="actor-filter">Actor (User ID)</label>
              <input
                type="text"
                id="actor-filter"
                value={actorFilter}
                onChange={(e) => setActorFilter(e.target.value)}
                placeholder="Filter by user ID"
                className="form-input"
              />
            </div>

            <div className="form-group">
              <label htmlFor="start-date">Start Date</label>
              <input
                type="date"
                id="start-date"
                value={startDate}
                onChange={(e) => setStartDate(e.target.value)}
                className="form-input"
              />
            </div>

            <div className="form-group">
              <label htmlFor="end-date">End Date</label>
              <input
                type="date"
                id="end-date"
                value={endDate}
                onChange={(e) => setEndDate(e.target.value)}
                className="form-input"
              />
            </div>
          </div>

          <div className="filter-actions">
            <Button onClick={handleResetFilters} variant="ghost">
              Reset
            </Button>
            <Button onClick={handleApplyFilters}>Apply Filters</Button>
          </div>
        </div>
      )}

      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      {logs.length === 0 ? (
        <EmptyState
          icon={
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M12 8v4l3 3M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          }
          title={`No audit logs found${hasActiveFilters ? ' matching the selected filters' : ''}.`}
          description="Member, invitation, workspace and AI settings changes are recorded here."
          action={
            hasActiveFilters ? (
              <Button onClick={handleResetFilters} variant="secondary">
                Clear Filters
              </Button>
            ) : undefined
          }
        />
      ) : (
        <>
          <div className="audit-logs-table-wrapper">
            <table className="audit-logs-table">
              <colgroup>
                <col className="audit-col-time" />
                <col className="audit-col-actor" />
                <col className="audit-col-action" />
                <col className="audit-col-type" />
                <col className="audit-col-id" />
                <col className="audit-col-details" />
              </colgroup>
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Actor</th>
                  <th>Action</th>
                  <th>Resource Type</th>
                  <th>Resource ID</th>
                  <th>Details</th>
                </tr>
              </thead>
              <tbody>
                {logs.map((log) => (
                  <Fragment key={log.id}>
                    <tr className="audit-log-row">
                      <td className="timestamp-cell" title={log.created_at}>
                        <span className="timestamp-relative">
                          {formatTimestamp(log.created_at).relative}
                        </span>
                        <span className="timestamp-absolute">
                          {formatTimestamp(log.created_at).absolute}
                        </span>
                      </td>
                      <td className="actor-cell">
                        <div className="actor-info">
                          <span className="actor-email">{log.actor_email ?? 'System'}</span>
                          <span className="actor-id">{log.actor_id ?? ''}</span>
                        </div>
                      </td>
                      <td className="action-cell">
                        <span className={`action-badge ${getActionBadgeClass(log.action)}`}>
                          {log.action}
                        </span>
                      </td>
                      <td className="resource-type-cell">{log.resource_type}</td>
                      <td className="resource-id-cell">
                        <code title={log.resource_id ?? undefined}>{log.resource_id ?? '—'}</code>
                      </td>
                      <td className="details-cell">
                        <Button
                          type="button"
                          onClick={() => toggleExpanded(log.id)}
                          className="expand-button"
                          variant="ghost"
                          size="sm"
                          aria-expanded={expandedLog === log.id}
                        >
                          {expandedLog === log.id ? 'Hide' : 'Show'}
                        </Button>
                      </td>
                    </tr>
                    {expandedLog === log.id && (
                      <tr className="metadata-row">
                        <td colSpan={6}>
                          <div className="metadata-content">
                            <h4>Recorded values</h4>
                            <pre>
                              {JSON.stringify(
                                {
                                  workspace_id: log.workspace_id,
                                  old_values: log.old_values,
                                  new_values: log.new_values,
                                },
                                null,
                                2
                              )}
                            </pre>
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                ))}
              </tbody>
            </table>
          </div>

          {hasMore && (
            <div className="load-more-section">
              <Button onClick={handleLoadMore} loading={loading} variant="ghost" size="sm">
                Load More ({total - offset - logs.length} remaining)
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

import { Button } from '@zone/ui';
import { Fragment, useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import type { AuditAction, AuditLog, AuditLogFilters, AuditResourceType } from '../types';
import './AuditLogsSection.css';

interface AuditLogsSectionProps {
  orgId: string;
}

const ACTIONS: AuditAction[] = [
  'create',
  'update',
  'delete',
  'login',
  'logout',
  'invite',
  'accept',
  'revoke',
];
const RESOURCE_TYPES: AuditResourceType[] = [
  'user',
  'organization',
  'workspace',
  'project',
  'task',
  'source',
  'chat',
  'invitation',
  'member',
];

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

  const formatTimestamp = (timestamp: string): string => {
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

    return `${absolute} (${relative})`;
  };

  const getActionBadgeClass = (action: AuditAction): string => {
    switch (action) {
      case 'create':
        return 'action-create';
      case 'update':
        return 'action-update';
      case 'delete':
        return 'action-delete';
      case 'login':
      case 'accept':
        return 'action-success';
      case 'logout':
      case 'revoke':
        return 'action-warning';
      default:
        return 'action-default';
    }
  };

  if (loading && offset === 0) {
    return <div className="audit-logs-loading">Loading audit logs...</div>;
  }

  if (error && offset === 0) {
    return (
      <div className="audit-logs-error">
        <p>{error}</p>
        <Button onClick={loadLogs} variant="secondary">
          Retry
        </Button>
      </div>
    );
  }

  const hasMore = offset + logs.length < total;
  const hasActiveFilters = action || resourceType || actorFilter || startDate || endDate;

  return (
    <div className="audit-logs-section">
      <div className="audit-logs-header">
        <div>
          <h2 className="section-title">Audit Logs</h2>
          <p className="section-description">
            View activity history for your organization. {total} total{' '}
            {total === 1 ? 'entry' : 'entries'}.
          </p>
        </div>
        <div className="audit-logs-actions">
          <Button onClick={() => setShowFilters(!showFilters)} variant="secondary">
            {showFilters ? 'Hide Filters' : 'Show Filters'}
          </Button>
          <Button onClick={handleExport} loading={exporting} variant="primary">
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
            <Button onClick={handleResetFilters} variant="secondary">
              Reset
            </Button>
            <Button onClick={handleApplyFilters} variant="primary">
              Apply Filters
            </Button>
          </div>
        </div>
      )}

      {error && <div className="audit-logs-error">{error}</div>}

      {logs.length === 0 ? (
        <div className="empty-state">
          <p>No audit logs found{hasActiveFilters ? ' matching the selected filters' : ''}.</p>
          {hasActiveFilters && (
            <Button onClick={handleResetFilters} variant="secondary">
              Clear Filters
            </Button>
          )}
        </div>
      ) : (
        <>
          <div className="audit-logs-table-wrapper">
            <table className="audit-logs-table">
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
                      <td className="timestamp-cell">
                        <span title={log.created_at}>{formatTimestamp(log.created_at)}</span>
                      </td>
                      <td className="actor-cell">
                        <div className="actor-info">
                          <span className="actor-email">{log.actor_email}</span>
                          <span className="actor-id">{log.actor_id}</span>
                        </div>
                      </td>
                      <td className="action-cell">
                        <span className={`action-badge ${getActionBadgeClass(log.action)}`}>
                          {log.action}
                        </span>
                      </td>
                      <td className="resource-type-cell">{log.resource_type}</td>
                      <td className="resource-id-cell">
                        <code>{log.resource_id}</code>
                      </td>
                      <td className="details-cell">
                        <button
                          type="button"
                          onClick={() => toggleExpanded(log.id)}
                          className="expand-button"
                          aria-expanded={expandedLog === log.id}
                        >
                          {expandedLog === log.id ? 'Hide' : 'Show'}
                        </button>
                      </td>
                    </tr>
                    {expandedLog === log.id && (
                      <tr className="metadata-row">
                        <td colSpan={6}>
                          <div className="metadata-content">
                            <h4>Metadata</h4>
                            <pre>{JSON.stringify(log.metadata, null, 2)}</pre>
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
              <Button onClick={handleLoadMore} loading={loading} variant="secondary">
                Load More ({total - offset - logs.length} remaining)
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

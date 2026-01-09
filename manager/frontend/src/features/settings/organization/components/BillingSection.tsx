import { useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import type { Limits, Subscription, Usage } from '../types';
import './BillingSection.css';

interface BillingSectionProps {
  orgId: string;
}

interface UsageMetric {
  label: string;
  current: number;
  limit: number | null;
  unit: string;
  percentage: number;
}

export function BillingSection({ orgId }: BillingSectionProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [subscription, setSubscription] = useState<Subscription | null>(null);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [limits, setLimits] = useState<Limits | null>(null);

  const loadBillingData = useCallback(async () => {
    if (!orgId) return;
    setLoading(true);
    setError(null);

    try {
      const [subData, usageData, limitsData] = await Promise.all([
        client.getSubscription(orgId),
        client.getUsage(orgId),
        client.getLimits(orgId),
      ]);
      setSubscription(subData);
      setUsage(usageData);
      setLimits(limitsData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load billing data');
    } finally {
      setLoading(false);
    }
  }, [orgId]);

  useEffect(() => {
    loadBillingData();
  }, [loadBillingData]);

  const calculatePercentage = (current: number, limit: number | null): number => {
    if (limit === null) return 0;
    return limit > 0 ? Math.min((current / limit) * 100, 100) : 0;
  };

  const getProgressColor = (percentage: number): string => {
    if (percentage >= 95) return 'critical';
    if (percentage >= 80) return 'warning';
    return 'ok';
  };

  const formatDate = (dateString: string): string => {
    return new Date(dateString).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    });
  };

  const formatNumber = (num: number): string => {
    return new Intl.NumberFormat('en-US').format(num);
  };

  const getStatusBadgeClass = (status: Subscription['status']): string => {
    switch (status) {
      case 'active':
        return 'status-active';
      case 'trialing':
        return 'status-trialing';
      case 'past_due':
        return 'status-past-due';
      case 'canceled':
        return 'status-canceled';
      default:
        return '';
    }
  };

  if (loading) {
    return <div className="billing-loading">Loading billing information...</div>;
  }

  if (error) {
    return (
      <div className="billing-error">
        <p>{error}</p>
        <button type="button" onClick={loadBillingData} className="retry-button">
          Retry
        </button>
      </div>
    );
  }

  if (!subscription || !usage || !limits) {
    return <div className="billing-error">No billing data available</div>;
  }

  const metrics: UsageMetric[] = [
    {
      label: 'Users',
      current: usage.users,
      limit: limits.max_users,
      unit: 'users',
      percentage: calculatePercentage(usage.users, limits.max_users),
    },
    {
      label: 'Workspaces',
      current: usage.workspaces,
      limit: limits.max_workspaces,
      unit: 'workspaces',
      percentage: calculatePercentage(usage.workspaces, limits.max_workspaces),
    },
    {
      label: 'Projects',
      current: usage.projects,
      limit: limits.max_projects,
      unit: 'projects',
      percentage: calculatePercentage(usage.projects, limits.max_projects),
    },
    {
      label: 'Storage',
      current: usage.storage_gb,
      limit: limits.max_storage_gb,
      unit: 'GB',
      percentage: calculatePercentage(usage.storage_gb, limits.max_storage_gb),
    },
    {
      label: 'API Calls',
      current: usage.api_calls,
      limit: limits.max_api_calls_monthly,
      unit: 'calls',
      percentage: calculatePercentage(usage.api_calls, limits.max_api_calls_monthly),
    },
  ];

  return (
    <div className="billing-section">
      <section className="subscription-info">
        <h2 className="section-title">Current Subscription</h2>
        <div className="subscription-card">
          <div className="subscription-header">
            <h3 className="plan-name">{subscription.plan_name}</h3>
            <span className={`status-badge ${getStatusBadgeClass(subscription.status)}`}>
              {subscription.status}
            </span>
          </div>
          <div className="subscription-details">
            <div className="detail-row">
              <span className="detail-label">Billing Period:</span>
              <span className="detail-value">
                {formatDate(subscription.current_period_start)} -{' '}
                {formatDate(subscription.current_period_end)}
              </span>
            </div>
            {subscription.cancel_at_period_end && (
              <div className="cancel-warning">
                Subscription will be canceled at the end of the current billing period.
              </div>
            )}
          </div>
        </div>
      </section>

      <section className="usage-dashboard">
        <h2 className="section-title">Usage & Limits</h2>
        <p className="usage-period">
          Current period: {formatDate(usage.period_start)} - {formatDate(usage.period_end)}
        </p>

        <div className="metrics-grid">
          {metrics.map((metric) => (
            <div key={metric.label} className="metric-card">
              <div className="metric-header">
                <h3 className="metric-label">{metric.label}</h3>
                {metric.percentage >= 95 && (
                  <span className="metric-warning-badge critical">Limit Reached</span>
                )}
                {metric.percentage >= 80 && metric.percentage < 95 && (
                  <span className="metric-warning-badge warning">Near Limit</span>
                )}
              </div>

              <div className="metric-value">
                <span className="current-value">{formatNumber(metric.current)}</span>
                {metric.limit !== null && (
                  <span className="limit-value">
                    {' '}
                    / {formatNumber(metric.limit)} {metric.unit}
                  </span>
                )}
                {metric.limit === null && <span className="unlimited-label"> (unlimited)</span>}
              </div>

              {metric.limit !== null && (
                <>
                  <div className="progress-bar">
                    <div
                      className={`progress-fill ${getProgressColor(metric.percentage)}`}
                      style={{ width: `${metric.percentage}%` }}
                      role="progressbar"
                      aria-valuenow={metric.percentage}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-label={`${metric.label} usage`}
                    />
                  </div>
                  <div className="percentage-label">{metric.percentage.toFixed(1)}% used</div>
                </>
              )}
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

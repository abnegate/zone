import type React from 'react';
import { useFormContext } from 'react-hook-form';
import { Checkbox, InfoBox, Input, Select } from '../components';
import { useSecretGenerator } from '../hooks';
import type { InstallerConfig } from '../types';

const retentionOptions = [
  { value: '7d', label: '7 days' },
  { value: '15d', label: '15 days' },
  { value: '30d', label: '30 days' },
  { value: '90d', label: '90 days' },
];

const timezoneOptions = [
  { value: 'UTC', label: 'UTC' },
  { value: 'America/New_York', label: 'America/New_York' },
  { value: 'America/Los_Angeles', label: 'America/Los_Angeles' },
  { value: 'Europe/London', label: 'Europe/London' },
  { value: 'Asia/Tokyo', label: 'Asia/Tokyo' },
];

const smtpPortOptions = [
  { value: '25', label: '25 (SMTP)' },
  { value: '465', label: '465 (SMTPS)' },
  { value: '587', label: '587 (Submission)' },
  { value: '2525', label: '2525 (Alternate)' },
];

export function AdvancedStep() {
  const {
    register,
    setValue,
    watch,
    formState: { errors },
  } = useFormContext<InstallerConfig>();
  const { generateSecret } = useSecretGenerator();
  const monitoringEnabled = watch('MONITORING_ENABLED') === 'true';
  const alertingEnabled = watch('ALERT_ENABLED') === 'true';
  const grafanaPassword = watch('MONITORING_GRAFANA_ADMIN_PASSWORD');

  const handleMonitoringToggle = (checked: boolean) => {
    setValue('MONITORING_ENABLED', checked ? 'true' : 'false', {
      shouldDirty: true,
      shouldValidate: true,
    });
    // Auto-generate password if enabling and password is empty
    if (checked && !grafanaPassword) {
      setValue('MONITORING_GRAFANA_ADMIN_PASSWORD', generateSecret(), {
        shouldDirty: true,
        shouldValidate: true,
      });
    }
    // Disable alerting if monitoring is disabled
    if (!checked) {
      setValue('ALERT_ENABLED', 'false', { shouldDirty: true, shouldValidate: true });
    }
  };

  const handleAlertingToggle = (checked: boolean) => {
    setValue('ALERT_ENABLED', checked ? 'true' : 'false', {
      shouldDirty: true,
      shouldValidate: true,
    });
  };

  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Advanced Settings</h2>
        <p>Performance tuning and system configuration</p>
      </div>

      <h3 className="section-header">Monitoring</h3>

      <div className="form-field">
        <Checkbox
          label="Enable Prometheus + Grafana monitoring"
          checked={monitoringEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            handleMonitoringToggle(e.target.checked)
          }
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Adds metrics collection and dashboards
        </p>
      </div>

      {monitoringEnabled && (
        <div className="conditional-fields">
          <Input
            label="Grafana Admin Username"
            type="text"
            error={errors.MONITORING_GRAFANA_ADMIN_USER?.message}
            {...register('MONITORING_GRAFANA_ADMIN_USER')}
          />

          <Input
            label="Grafana Admin Password"
            type="text"
            onGenerate={() =>
              setValue('MONITORING_GRAFANA_ADMIN_PASSWORD', generateSecret(), {
                shouldDirty: true,
                shouldValidate: true,
              })
            }
            placeholder="Leave empty to auto-generate"
            className="font-mono"
            error={errors.MONITORING_GRAFANA_ADMIN_PASSWORD?.message}
            {...register('MONITORING_GRAFANA_ADMIN_PASSWORD')}
          />

          <Select
            label="Metrics Retention"
            options={retentionOptions}
            helpText="How long to keep metrics data"
            {...register('MONITORING_RETENTION_TIME')}
          />

          <InfoBox variant="info">
            Start with:{' '}
            <code
              style={{
                background: 'var(--bg-base)',
                padding: '0.25rem 0.5rem',
                borderRadius: '0.25rem',
              }}
            >
              docker compose --profile monitoring up
            </code>
          </InfoBox>

          <h4 className="section-header" style={{ marginTop: 'var(--space-lg)' }}>
            Email Alerts
          </h4>

          <div className="form-field">
            <Checkbox
              label="Enable email alerts for critical events"
              checked={alertingEnabled}
              onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
                handleAlertingToggle(e.target.checked)
              }
            />
            <p className="help-text" style={{ marginLeft: '2.25rem' }}>
              Get notified when services go down or performance degrades
            </p>
          </div>

          {alertingEnabled && (
            <div className="conditional-fields">
              <Input
                label="Alert Recipients"
                type="email"
                placeholder="admin@example.com"
                helpText="Comma-separated list of email addresses"
                error={errors.ALERT_EMAIL_RECIPIENTS?.message}
                {...register('ALERT_EMAIL_RECIPIENTS')}
              />

              <Input
                label="SMTP Host"
                type="text"
                placeholder="smtp.gmail.com"
                helpText="e.g., smtp.gmail.com, smtp.sendgrid.net"
                error={errors.ALERT_SMTP_HOST?.message}
                {...register('ALERT_SMTP_HOST')}
              />

              <Select
                label="SMTP Port"
                options={smtpPortOptions}
                helpText="587 recommended for most providers"
                {...register('ALERT_SMTP_PORT')}
              />

              <Input
                label="SMTP Username"
                type="text"
                placeholder="your-email@gmail.com"
                error={errors.ALERT_SMTP_USER?.message}
                {...register('ALERT_SMTP_USER')}
              />

              <Input
                label="SMTP Password"
                type="password"
                placeholder="App password or API key"
                helpText="For Gmail, use an App Password"
                error={errors.ALERT_SMTP_PASSWORD?.message}
                {...register('ALERT_SMTP_PASSWORD')}
              />

              <Input
                label="From Address"
                type="email"
                placeholder="alerts@example.com"
                error={errors.ALERT_SMTP_FROM_ADDRESS?.message}
                {...register('ALERT_SMTP_FROM_ADDRESS')}
              />

              <Input
                label="From Name"
                type="text"
                placeholder="Zone Alerts"
                error={errors.ALERT_SMTP_FROM_NAME?.message}
                {...register('ALERT_SMTP_FROM_NAME')}
              />

              <InfoBox variant="info">
                Alerts include: service outages, high latency, error spikes, database issues, and
                memory warnings.
              </InfoBox>
            </div>
          )}
        </div>
      )}

      <h3 className="section-header">Performance</h3>

      <Input
        label="Worker Count"
        type="number"
        min={1}
        max={16}
        helpText="1-2 per CPU core recommended"
        error={errors.ADVANCED_LITELLM_WORKERS?.message}
        {...register('ADVANCED_LITELLM_WORKERS')}
      />

      <Input
        label="Request Timeout (seconds)"
        type="number"
        min={60}
        max={1800}
        error={errors.ADVANCED_LITELLM_REQUEST_TIMEOUT?.message}
        {...register('ADVANCED_LITELLM_REQUEST_TIMEOUT')}
      />

      <Select
        label="Timezone"
        options={timezoneOptions}
        {...register('ADVANCED_TZ')}
      />

      <Input
        label="ACME Email (for Let's Encrypt)"
        type="email"
        helpText="Required for automatic TLS certificates"
        error={errors.ADVANCED_ACME_EMAIL?.message}
        {...register('ADVANCED_ACME_EMAIL')}
      />

      <InfoBox variant="success">Configuration complete. Click Install to proceed.</InfoBox>
    </div>
  );
}

import type React from 'react';
import { Controller, useFormContext } from 'react-hook-form';
import {
  AlertDescription,
  AlertTitle,
  Checkbox,
  InfoBox,
  Input,
  SectionHeader,
  Select,
} from '../components';
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
    control,
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
    if (checked && !grafanaPassword) {
      setValue('MONITORING_GRAFANA_ADMIN_PASSWORD', generateSecret(), {
        shouldDirty: true,
        shouldValidate: true,
      });
    }
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
    <div className="space-y-6">
      <div className="space-y-4">
        <SectionHeader title="Monitoring" />
        <Checkbox
          label="Enable Prometheus + Grafana monitoring"
          checked={monitoringEnabled}
          helpText="Adds metrics collection and dashboards"
          onChange={(e: React.ChangeEvent<HTMLInputElement>) => handleMonitoringToggle(e.target.checked)}
        />

        {monitoringEnabled && (
          <div className="space-y-4 rounded-md border border-dashed p-4">
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

            <Controller
              control={control}
              name="MONITORING_RETENTION_TIME"
              render={({ field }) => (
                <Select
                  label="Metrics Retention"
                  options={retentionOptions}
                  helpText="How long to keep metrics data"
                  value={field.value}
                  onValueChange={field.onChange}
                  name={field.name}
                />
              )}
            />

            <InfoBox variant="info">
              <AlertDescription className="flex flex-wrap items-center gap-2">
                <span>Start with:</span>
                <code className="rounded-md bg-muted px-2 py-1 text-xs">
                  docker compose --profile monitoring up
                </code>
              </AlertDescription>
            </InfoBox>

            <div className="space-y-3">
              <SectionHeader title="Email Alerts" size="sm" />
              <Checkbox
                label="Enable email alerts for critical events"
                checked={alertingEnabled}
                helpText="Get notified when services go down or performance degrades"
                onChange={(e: React.ChangeEvent<HTMLInputElement>) => handleAlertingToggle(e.target.checked)}
              />

              {alertingEnabled && (
                <div className="space-y-4 rounded-md border border-dashed p-4">
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

                  <Controller
                    control={control}
                    name="ALERT_SMTP_PORT"
                    render={({ field }) => (
                      <Select
                        label="SMTP Port"
                        options={smtpPortOptions}
                        helpText="587 recommended for most providers"
                        value={field.value}
                        onValueChange={field.onChange}
                        name={field.name}
                      />
                    )}
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
                    <AlertDescription>
                      Alerts include: service outages, high latency, error spikes, database issues,
                      and memory warnings.
                    </AlertDescription>
                  </InfoBox>
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      <div className="space-y-4">
        <SectionHeader title="Performance" />
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

        <Controller
          control={control}
          name="ADVANCED_TZ"
          render={({ field }) => (
            <Select
              label="Timezone"
              options={timezoneOptions}
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />

        <Input
          label="ACME Email (for Let's Encrypt)"
          type="email"
          helpText="Required for automatic TLS certificates"
          error={errors.ADVANCED_ACME_EMAIL?.message}
          {...register('ADVANCED_ACME_EMAIL')}
        />
      </div>

      <InfoBox variant="success">
        <AlertTitle>Ready to install</AlertTitle>
        <AlertDescription>Configuration complete. Click Install to proceed.</AlertDescription>
      </InfoBox>
    </div>
  );
}

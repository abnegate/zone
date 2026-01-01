import React from 'react';
import { Checkbox, Select, Input, InfoBox } from '../components';
import { useSecretGenerator } from '../hooks';
import type { InstallerConfig } from '../types';

interface AdvancedStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

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

export function AdvancedStep({ config, onChange }: AdvancedStepProps) {
  const { generateSecret } = useSecretGenerator();
  const monitoringEnabled = config.ENABLE_MONITORING === 'true';

  const handleMonitoringToggle = (checked: boolean) => {
    onChange('ENABLE_MONITORING', checked ? 'true' : 'false');
    // Auto-generate password if enabling and password is empty
    if (checked && !config.GF_SECURITY_ADMIN_PASSWORD) {
      onChange('GF_SECURITY_ADMIN_PASSWORD', generateSecret());
    }
  };

  return (
    <div className="step-content">
      <h2>Advanced Settings</h2>
      <p>Performance tuning and system configuration</p>

      <h3 className="section-header">Monitoring</h3>

      <div className="form-field">
        <Checkbox
          label="Enable Prometheus + Grafana monitoring"
          checked={monitoringEnabled}
          onChange={e => handleMonitoringToggle(e.target.checked)}
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
            value={config.GF_SECURITY_ADMIN_USER}
            onChange={e => onChange('GF_SECURITY_ADMIN_USER', e.target.value)}
          />

          <Input
            label="Grafana Admin Password"
            type="text"
            value={config.GF_SECURITY_ADMIN_PASSWORD}
            onChange={e => onChange('GF_SECURITY_ADMIN_PASSWORD', e.target.value)}
            onGenerate={() => onChange('GF_SECURITY_ADMIN_PASSWORD', generateSecret())}
            placeholder="Leave empty to auto-generate"
            className="font-mono"
          />

          <Select
            label="Metrics Retention"
            options={retentionOptions}
            value={config.METRICS_RETENTION}
            onChange={e => onChange('METRICS_RETENTION', e.target.value)}
            helpText="How long to keep metrics data"
          />

          <InfoBox variant="info">
            Start with: <code style={{ background: 'var(--bg-base)', padding: '0.25rem 0.5rem', borderRadius: '0.25rem' }}>docker compose --profile monitoring up</code>
          </InfoBox>
        </div>
      )}

      <h3 className="section-header">Performance</h3>

      <Input
        label="Worker Count"
        type="number"
        value={config.WORKERS}
        onChange={e => onChange('WORKERS', e.target.value)}
        min={1}
        max={16}
        helpText="1-2 per CPU core recommended"
      />

      <Input
        label="Request Timeout (seconds)"
        type="number"
        value={config.REQUEST_TIMEOUT}
        onChange={e => onChange('REQUEST_TIMEOUT', e.target.value)}
        min={60}
        max={1800}
      />

      <Select
        label="Timezone"
        options={timezoneOptions}
        value={config.TZ}
        onChange={e => onChange('TZ', e.target.value)}
      />

      <Input
        label="ACME Email (for Let's Encrypt)"
        type="email"
        value={config.ACME_EMAIL}
        onChange={e => onChange('ACME_EMAIL', e.target.value)}
        helpText="Required for automatic TLS certificates"
      />

      <InfoBox variant="success">
        Configuration complete. Click Install to proceed.
      </InfoBox>
    </div>
  );
}

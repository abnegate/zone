import React from 'react';
import { Input, InfoBox, Button, Checkbox } from '../components';
import { useSecretGenerator } from '../hooks';
import type { InstallerConfig } from '../types';

interface SecurityStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
}

export function SecurityStep({ config, onChange, getFieldError }: SecurityStepProps) {
  const { generateSecret } = useSecretGenerator();

  const handleGenerateAll = () => {
    onChange('SECURITY_LITELLM_MASTER_KEY', generateSecret());
    onChange('SECURITY_LITELLM_SALT_KEY', generateSecret());
    onChange('SECURITY_SEARXNG_SECRET_KEY', generateSecret());
    onChange('SECURITY_MANAGER_API_KEY', generateSecret());
    onChange('POSTGRES_PASSWORD', generateSecret());
  };

  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Security</h2>
        <p>Configure authentication and generate secure keys</p>
      </div>

      <Input
        label="Authentication Realm"
        type="text"
        value={config.SECURITY_BASICAUTH_REALM}
        onChange={e => onChange('SECURITY_BASICAUTH_REALM', e.target.value)}
        error={getFieldError('SECURITY_BASICAUTH_REALM')}
      />

      <Input
        label="LiteLLM Master Key"
        type="text"
        value={config.SECURITY_LITELLM_MASTER_KEY}
        onChange={e => onChange('SECURITY_LITELLM_MASTER_KEY', e.target.value)}
        onGenerate={() => onChange('SECURITY_LITELLM_MASTER_KEY', generateSecret())}
        className="font-mono"
        error={getFieldError('SECURITY_LITELLM_MASTER_KEY')}
      />

      <Input
        label="LiteLLM Salt Key"
        type="text"
        value={config.SECURITY_LITELLM_SALT_KEY}
        onChange={e => onChange('SECURITY_LITELLM_SALT_KEY', e.target.value)}
        onGenerate={() => onChange('SECURITY_LITELLM_SALT_KEY', generateSecret())}
        className="font-mono"
        error={getFieldError('SECURITY_LITELLM_SALT_KEY')}
      />

      <Input
        label="SearXNG Secret Key"
        type="text"
        value={config.SECURITY_SEARXNG_SECRET_KEY}
        onChange={e => onChange('SECURITY_SEARXNG_SECRET_KEY', e.target.value)}
        onGenerate={() => onChange('SECURITY_SEARXNG_SECRET_KEY', generateSecret())}
        className="font-mono"
        error={getFieldError('SECURITY_SEARXNG_SECRET_KEY')}
      />

      <Input
        label="Manager API Key"
        type="text"
        value={config.SECURITY_MANAGER_API_KEY}
        onChange={e => onChange('SECURITY_MANAGER_API_KEY', e.target.value)}
        onGenerate={() => onChange('SECURITY_MANAGER_API_KEY', generateSecret())}
        className="font-mono"
        error={getFieldError('SECURITY_MANAGER_API_KEY')}
      />

      <Input
        label="PostgreSQL Password"
        type="text"
        value={config.POSTGRES_PASSWORD}
        onChange={e => onChange('POSTGRES_PASSWORD', e.target.value)}
        onGenerate={() => onChange('POSTGRES_PASSWORD', generateSecret())}
        className="font-mono"
        error={getFieldError('POSTGRES_PASSWORD')}
      />

      <Button variant="generate" className="w-full" onClick={handleGenerateAll}>
        Generate All Secrets
      </Button>

      <h3 className="section-header">Production Settings</h3>

      <div className="form-field">
        <Checkbox
          label="Enable HTTPS redirect"
          checked={config.SECURITY_HTTP_REDIRECT === 'true'}
          onChange={e => onChange('SECURITY_HTTP_REDIRECT', e.target.checked ? 'true' : 'false')}
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Redirect HTTP to HTTPS (requires valid TLS certificate)
        </p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Auto-generate TLS certificate (Let's Encrypt)"
          checked={config.SECURITY_GENERATE_CERTIFICATE === 'true'}
          onChange={e => onChange('SECURITY_GENERATE_CERTIFICATE', e.target.checked ? 'true' : 'false')}
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Requires public domain and ports 80/443 accessible
        </p>
      </div>

      {config.SECURITY_GENERATE_CERTIFICATE === 'true' && (
        <InfoBox variant="info">
          Set your ACME email in Advanced settings for certificate notifications.
        </InfoBox>
      )}

      <div className="mt-md">
        <InfoBox variant="warning">
          <strong>Note:</strong> Empty keys are insecure. Generate new keys for production use.
        </InfoBox>
      </div>
    </div>
  );
}

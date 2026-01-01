import React from 'react';
import { Input, InfoBox, Button } from '../components';
import { useSecretGenerator } from '../hooks';
import type { InstallerConfig } from '../types';

interface SecurityStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

export function SecurityStep({ config, onChange }: SecurityStepProps) {
  const { generateSecret } = useSecretGenerator();

  const handleGenerateAll = () => {
    onChange('LITELLM_MASTER_KEY', generateSecret());
    onChange('LITELLM_SALT_KEY', generateSecret());
    onChange('SEARXNG_SECRET_KEY', generateSecret());
  };

  return (
    <div className="step-content">
      <h2>Security</h2>
      <p>Configure authentication and generate secure keys</p>

      <Input
        label="Authentication Realm"
        type="text"
        value={config.SECURITY_AUTH_REALM}
        onChange={e => onChange('SECURITY_AUTH_REALM', e.target.value)}
      />

      <Input
        label="LiteLLM Master Key"
        type="text"
        value={config.LITELLM_MASTER_KEY}
        onChange={e => onChange('LITELLM_MASTER_KEY', e.target.value)}
        onGenerate={() => onChange('LITELLM_MASTER_KEY', generateSecret())}
        className="font-mono"
      />

      <Input
        label="LiteLLM Salt Key"
        type="text"
        value={config.LITELLM_SALT_KEY}
        onChange={e => onChange('LITELLM_SALT_KEY', e.target.value)}
        onGenerate={() => onChange('LITELLM_SALT_KEY', generateSecret())}
        className="font-mono"
      />

      <Input
        label="SearXNG Secret Key"
        type="text"
        value={config.SEARXNG_SECRET_KEY}
        onChange={e => onChange('SEARXNG_SECRET_KEY', e.target.value)}
        onGenerate={() => onChange('SEARXNG_SECRET_KEY', generateSecret())}
        className="font-mono"
      />

      <Button variant="generate" className="w-full" onClick={handleGenerateAll}>
        Generate All Secrets
      </Button>

      <div className="mt-md">
        <InfoBox variant="warning">
          <strong>Note:</strong> Default keys are insecure. Generate new keys for production use.
        </InfoBox>
      </div>
    </div>
  );
}

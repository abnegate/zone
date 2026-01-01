import React from 'react';
import { Input } from '../components';
import type { InstallerConfig } from '../types';

interface DomainStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
}

export function DomainStep({ config, onChange, getFieldError }: DomainStepProps) {
  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Domain Configuration</h2>
        <p>Configure hostnames for your services</p>
      </div>

      <Input
        label="Web Interface Hostname"
        type="text"
        value={config.DOMAIN_HOST_WEBUI}
        onChange={e => onChange('DOMAIN_HOST_WEBUI', e.target.value)}
        placeholder="webui.localhost"
        helpText="Hostname for the chat interface"
        error={getFieldError('DOMAIN_HOST_WEBUI')}
      />
    </div>
  );
}

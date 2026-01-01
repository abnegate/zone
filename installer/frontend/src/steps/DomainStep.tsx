import React from 'react';
import { Input } from '../components';
import type { InstallerConfig } from '../types';

interface DomainStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

export function DomainStep({ config, onChange }: DomainStepProps) {
  return (
    <div className="step-content">
      <h2>Domain Configuration</h2>
      <p>Configure hostnames for your services</p>

      <Input
        label="Web Interface Hostname"
        type="text"
        value={config.WEBUI_HOSTNAME}
        onChange={e => onChange('WEBUI_HOSTNAME', e.target.value)}
        placeholder="webui.localhost"
        helpText="Hostname for the chat interface"
      />
    </div>
  );
}

import React from 'react';
import { Checkbox, Select } from '../components';
import type { InstallerConfig } from '../types';

interface InterfaceStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
}

const localeOptions = [
  { value: 'en-US', label: 'English (US)' },
  { value: 'en-GB', label: 'English (UK)' },
  { value: 'es-ES', label: 'Spanish' },
  { value: 'fr-FR', label: 'French' },
  { value: 'de-DE', label: 'German' },
  { value: 'ja-JP', label: 'Japanese' },
  { value: 'zh-CN', label: 'Chinese (Simplified)' },
];

export function InterfaceStep({ config, onChange, getFieldError }: InterfaceStepProps) {
  void getFieldError; // Interface uses checkboxes and selects, no validation errors shown
  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Interface Settings</h2>
        <p>Configure the web interface</p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Enable built-in authentication"
          checked={config.WEBUI_AUTH === 'true'}
          onChange={e => onChange('WEBUI_AUTH', e.target.checked ? 'true' : 'false')}
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Uses Traefik basic auth by default
        </p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Allow user signups"
          checked={config.WEBUI_ENABLE_SIGNUP === 'true'}
          onChange={e => onChange('WEBUI_ENABLE_SIGNUP', e.target.checked ? 'true' : 'false')}
        />
      </div>

      <Select
        label="Default Language"
        options={localeOptions}
        value={config.WEBUI_DEFAULT_LOCALE}
        onChange={e => onChange('WEBUI_DEFAULT_LOCALE', e.target.value)}
      />
    </div>
  );
}

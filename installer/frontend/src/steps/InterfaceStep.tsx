import type React from 'react';
import { useFormContext } from 'react-hook-form';
import { Checkbox, Select } from '../components';
import type { InstallerConfig } from '../types';

const localeOptions = [
  { value: 'en-US', label: 'English (US)' },
  { value: 'en-GB', label: 'English (UK)' },
  { value: 'es-ES', label: 'Spanish' },
  { value: 'fr-FR', label: 'French' },
  { value: 'de-DE', label: 'German' },
  { value: 'ja-JP', label: 'Japanese' },
  { value: 'zh-CN', label: 'Chinese (Simplified)' },
];

export function InterfaceStep() {
  const { register, setValue, watch } = useFormContext<InstallerConfig>();
  const authEnabled = watch('WEBUI_AUTH') === 'true';
  const signupEnabled = watch('WEBUI_ENABLE_SIGNUP') === 'true';
  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Interface Settings</h2>
        <p>Configure the web interface</p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Enable built-in authentication"
          checked={authEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('WEBUI_AUTH', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Uses Traefik basic auth by default
        </p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Allow user signups"
          checked={signupEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('WEBUI_ENABLE_SIGNUP', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />
      </div>

      <Select
        label="Default Language"
        options={localeOptions}
        {...register('WEBUI_DEFAULT_LOCALE')}
      />
    </div>
  );
}

import type React from 'react';
import { Controller, useFormContext } from 'react-hook-form';
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
  const { setValue, watch, control } = useFormContext<InstallerConfig>();
  const authEnabled = watch('WEBUI_AUTH') === 'true';
  const signupEnabled = watch('WEBUI_ENABLE_SIGNUP') === 'true';

  return (
    <div className="space-y-6">
      <div className="space-y-4">
        <Checkbox
          label="Enable built-in authentication"
          checked={authEnabled}
          helpText="Uses Traefik basic auth by default"
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('WEBUI_AUTH', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />

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

        <Controller
          control={control}
          name="WEBUI_DEFAULT_LOCALE"
          render={({ field }) => (
            <Select
              label="Default Language"
              options={localeOptions}
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />
      </div>
    </div>
  );
}

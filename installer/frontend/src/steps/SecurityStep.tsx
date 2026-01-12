import type React from 'react';
import { useFormContext } from 'react-hook-form';
import { Button, Checkbox, InfoBox, Input } from '../components';
import { useSecretGenerator } from '../hooks';
import type { InstallerConfig } from '../types';

export function SecurityStep() {
  const {
    register,
    setValue,
    watch,
    formState: { errors },
  } = useFormContext<InstallerConfig>();
  const { generateSecret } = useSecretGenerator();
  const httpRedirectEnabled = watch('SECURITY_HTTP_REDIRECT') === 'true';
  const certificateEnabled = watch('SECURITY_GENERATE_CERTIFICATE') === 'true';

  const handleGenerateAll = () => {
    const options = { shouldDirty: true, shouldValidate: true };
    setValue('SECURITY_LITELLM_MASTER_KEY', generateSecret(), options);
    setValue('SECURITY_LITELLM_SALT_KEY', generateSecret(), options);
    setValue('SECURITY_SEARXNG_SECRET_KEY', generateSecret(), options);
    setValue('SECURITY_MANAGER_API_KEY', generateSecret(), options);
    setValue('POSTGRES_PASSWORD', generateSecret(), options);
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
        error={errors.SECURITY_BASICAUTH_REALM?.message}
        {...register('SECURITY_BASICAUTH_REALM')}
      />

      <Input
        label="LiteLLM Master Key"
        type="text"
        onGenerate={() =>
          setValue('SECURITY_LITELLM_MASTER_KEY', generateSecret(), {
            shouldDirty: true,
            shouldValidate: true,
          })
        }
        className="font-mono"
        error={errors.SECURITY_LITELLM_MASTER_KEY?.message}
        {...register('SECURITY_LITELLM_MASTER_KEY')}
      />

      <Input
        label="LiteLLM Salt Key"
        type="text"
        onGenerate={() =>
          setValue('SECURITY_LITELLM_SALT_KEY', generateSecret(), {
            shouldDirty: true,
            shouldValidate: true,
          })
        }
        className="font-mono"
        error={errors.SECURITY_LITELLM_SALT_KEY?.message}
        {...register('SECURITY_LITELLM_SALT_KEY')}
      />

      <Input
        label="SearXNG Secret Key"
        type="text"
        onGenerate={() =>
          setValue('SECURITY_SEARXNG_SECRET_KEY', generateSecret(), {
            shouldDirty: true,
            shouldValidate: true,
          })
        }
        className="font-mono"
        error={errors.SECURITY_SEARXNG_SECRET_KEY?.message}
        {...register('SECURITY_SEARXNG_SECRET_KEY')}
      />

      <Input
        label="Manager API Key"
        type="text"
        onGenerate={() =>
          setValue('SECURITY_MANAGER_API_KEY', generateSecret(), {
            shouldDirty: true,
            shouldValidate: true,
          })
        }
        className="font-mono"
        error={errors.SECURITY_MANAGER_API_KEY?.message}
        {...register('SECURITY_MANAGER_API_KEY')}
      />

      <Input
        label="PostgreSQL Password"
        type="text"
        onGenerate={() =>
          setValue('POSTGRES_PASSWORD', generateSecret(), {
            shouldDirty: true,
            shouldValidate: true,
          })
        }
        className="font-mono"
        error={errors.POSTGRES_PASSWORD?.message}
        {...register('POSTGRES_PASSWORD')}
      />

      <Button variant="generate" className="w-full" onClick={handleGenerateAll}>
        Generate All Secrets
      </Button>

      <h3 className="section-header">Production Settings</h3>

      <div className="form-field">
        <Checkbox
          label="Enable HTTPS redirect"
          checked={httpRedirectEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('SECURITY_HTTP_REDIRECT', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Redirect HTTP to HTTPS (requires valid TLS certificate)
        </p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Auto-generate TLS certificate (Let's Encrypt)"
          checked={certificateEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('SECURITY_GENERATE_CERTIFICATE', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />
        <p className="help-text" style={{ marginLeft: '2.25rem' }}>
          Requires public domain and ports 80/443 accessible
        </p>
      </div>

      {certificateEnabled && (
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

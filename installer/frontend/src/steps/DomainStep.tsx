import { useFormContext } from 'react-hook-form';
import { Input } from '../components';
import type { InstallerConfig } from '../types';

export function DomainStep() {
  const {
    register,
    formState: { errors },
  } = useFormContext<InstallerConfig>();

  return (
    <div className="space-y-6">
      <Input
        label="Web Interface Hostname"
        type="text"
        {...register('DOMAIN_HOST_WEBUI')}
        placeholder="webui.localhost"
        helpText="Hostname for the chat interface"
        error={errors.DOMAIN_HOST_WEBUI?.message}
      />
    </div>
  );
}

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
        label="Stack Domain"
        type="text"
        {...register('DOMAIN_HOST_WEBUI')}
        placeholder="webui.localhost"
        helpText="Base domain for Zone services; chat opens at manager.<domain>/chats"
        error={errors.DOMAIN_HOST_WEBUI?.message}
      />
    </div>
  );
}

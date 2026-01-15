import type React from 'react';
import { useFormContext } from 'react-hook-form';
import { AlertDescription, Checkbox, InfoBox, Input } from '../components';
import type { InstallerConfig } from '../types';

export function SearchStep() {
  const {
    register,
    setValue,
    watch,
    formState: { errors },
  } = useFormContext<InstallerConfig>();
  const webSearchEnabled = watch('SEARCH_ENABLE_WEB_SEARCH') === 'true';
  return (
    <div className="space-y-6">
      <div className="space-y-4">
        <Checkbox
          label="Enable web search in RAG pipeline"
          checked={webSearchEnabled}
          onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
            setValue('SEARCH_ENABLE_WEB_SEARCH', e.target.checked ? 'true' : 'false', {
              shouldDirty: true,
              shouldValidate: true,
            })
          }
        />

        <Input
          label="Results per Query"
          type="number"
          min={1}
          max={20}
          error={errors.SEARCH_RESULT_COUNT?.message}
          {...register('SEARCH_RESULT_COUNT')}
        />

        <Input
          label="Concurrent Requests"
          type="number"
          min={1}
          max={32}
          error={errors.SEARCH_CONCURRENT_REQUESTS?.message}
          {...register('SEARCH_CONCURRENT_REQUESTS')}
        />

        <Input
          label="Search Instance Name"
          type="text"
          error={errors.SEARCH_SEARXNG_INSTANCE_NAME?.message}
          {...register('SEARCH_SEARXNG_INSTANCE_NAME')}
        />
      </div>

      <InfoBox variant="info">
        <AlertDescription>Web search requires VPN configuration in the next step.</AlertDescription>
      </InfoBox>
    </div>
  );
}

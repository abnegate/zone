import React from 'react';
import { Checkbox, Input, InfoBox } from '../components';
import type { InstallerConfig } from '../types';

interface SearchStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
}

export function SearchStep({ config, onChange, getFieldError }: SearchStepProps) {
  return (
    <div className="step-content">
      <div className="step-header">
        <h2>Web Search</h2>
        <p>Configure search integration</p>
      </div>

      <div className="form-field">
        <Checkbox
          label="Enable web search in RAG pipeline"
          checked={config.SEARCH_ENABLE_WEB_SEARCH === 'true'}
          onChange={e => onChange('SEARCH_ENABLE_WEB_SEARCH', e.target.checked ? 'true' : 'false')}
        />
      </div>

      <Input
        label="Results per Query"
        type="number"
        value={config.SEARCH_RESULT_COUNT}
        onChange={e => onChange('SEARCH_RESULT_COUNT', e.target.value)}
        min={1}
        max={20}
        error={getFieldError('SEARCH_RESULT_COUNT')}
      />

      <Input
        label="Concurrent Requests"
        type="number"
        value={config.SEARCH_CONCURRENT_REQUESTS}
        onChange={e => onChange('SEARCH_CONCURRENT_REQUESTS', e.target.value)}
        min={1}
        max={32}
        error={getFieldError('SEARCH_CONCURRENT_REQUESTS')}
      />

      <Input
        label="Search Instance Name"
        type="text"
        value={config.SEARCH_SEARXNG_INSTANCE_NAME}
        onChange={e => onChange('SEARCH_SEARXNG_INSTANCE_NAME', e.target.value)}
        error={getFieldError('SEARCH_SEARXNG_INSTANCE_NAME')}
      />

      <InfoBox variant="info">
        Web search requires VPN configuration in the next step.
      </InfoBox>
    </div>
  );
}

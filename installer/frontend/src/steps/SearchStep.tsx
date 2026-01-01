import React from 'react';
import { Checkbox, Input, InfoBox } from '../components';
import type { InstallerConfig } from '../types';

interface SearchStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

export function SearchStep({ config, onChange }: SearchStepProps) {
  return (
    <div className="step-content">
      <h2>Web Search</h2>
      <p>Configure search integration</p>

      <div className="form-field">
        <Checkbox
          label="Enable web search in RAG pipeline"
          checked={config.ENABLE_RAG_WEB_SEARCH === 'true'}
          onChange={e => onChange('ENABLE_RAG_WEB_SEARCH', e.target.checked ? 'true' : 'false')}
        />
      </div>

      <Input
        label="Results per Query"
        type="number"
        value={config.RAG_WEB_SEARCH_RESULT_COUNT}
        onChange={e => onChange('RAG_WEB_SEARCH_RESULT_COUNT', e.target.value)}
        min={1}
        max={20}
      />

      <Input
        label="Concurrent Requests"
        type="number"
        value={config.RAG_WEB_SEARCH_CONCURRENT_REQUESTS}
        onChange={e => onChange('RAG_WEB_SEARCH_CONCURRENT_REQUESTS', e.target.value)}
        min={1}
        max={32}
      />

      <Input
        label="Search Instance Name"
        type="text"
        value={config.SEARXNG_INSTANCE_NAME}
        onChange={e => onChange('SEARXNG_INSTANCE_NAME', e.target.value)}
      />

      <InfoBox variant="info">
        Web search requires VPN configuration in the next step.
      </InfoBox>
    </div>
  );
}

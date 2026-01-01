import React from 'react';
import { Select, InfoBox } from '../components';
import type { InstallerConfig } from '../types';

interface ModelsStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

const fastModelOptions = [
  { value: 'llama3.2:3b', label: 'llama3.2:3b - 3GB (Very Fast)' },
  { value: 'llama3.1:8b', label: 'llama3.1:8b - 4.7GB (Recommended)' },
  { value: 'qwen2.5:7b', label: 'qwen2.5:7b - 4.4GB' },
  { value: 'mistral:7b', label: 'mistral:7b - 4.1GB' },
];

const reasoningModelOptions = [
  { value: 'deepseek-r1:7b', label: 'deepseek-r1:7b - 4.9GB' },
  { value: 'deepseek-r1:14b', label: 'deepseek-r1:14b - 8.9GB' },
  { value: 'deepseek-r1:32b', label: 'deepseek-r1:32b - 20GB (Best)' },
  { value: 'llama3.1:70b', label: 'llama3.1:70b - 40GB' },
];

const embeddingModelOptions = [
  { value: 'nomic-embed-text', label: 'nomic-embed-text - 274MB' },
  { value: 'mxbai-embed-large', label: 'mxbai-embed-large - 669MB' },
];

export function ModelsStep({ config, onChange }: ModelsStepProps) {
  return (
    <div className="step-content">
      <h2>Model Selection</h2>
      <p>Choose AI models based on your hardware</p>

      <Select
        label="Fast Model (4-8GB RAM)"
        options={fastModelOptions}
        value={config.OLLAMA_FAST_MODEL}
        onChange={e => onChange('OLLAMA_FAST_MODEL', e.target.value)}
        helpText="For general queries and quick responses"
      />

      <Select
        label="Reasoning Model (8-32GB RAM)"
        options={reasoningModelOptions}
        value={config.OLLAMA_REASONING_MODEL}
        onChange={e => onChange('OLLAMA_REASONING_MODEL', e.target.value)}
        helpText="For complex analysis and detailed reasoning"
      />

      <Select
        label="Embedding Model (1-2GB RAM)"
        options={embeddingModelOptions}
        value={config.OLLAMA_EMBEDDING_MODEL}
        onChange={e => onChange('OLLAMA_EMBEDDING_MODEL', e.target.value)}
        helpText="For semantic routing and search"
      />

      <InfoBox variant="info">
        Models will download on first start. Total size varies (typically 10-50GB).
      </InfoBox>
    </div>
  );
}

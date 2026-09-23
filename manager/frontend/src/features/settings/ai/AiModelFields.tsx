import type { AiProvider } from '../workspace/types';
import {
  AUDIO_MODEL_OPTIONS,
  comfyImageOptions,
  type InstalledModelOption,
  isAgentProvider,
  type ModelSelection,
  VIDEO_MODEL_OPTIONS,
  withCurrent,
} from './options';

const AGENT_EMBEDDING_HINT =
  "Coding agents have no embedding models, so this server's own embedding engine indexes sources and knowledge.";

function embeddingHint(provider: AiProvider, listed: boolean): string {
  if (isAgentProvider(provider)) return AGENT_EMBEDDING_HINT;
  if (listed) return 'Used to index sources and knowledge for retrieval.';
  if (provider === 'anthropic') {
    return 'Anthropic does not provide embedding models. Name one from another provider, such as OpenAI text-embedding-3-small.';
  }
  return 'Enter a custom embedding model name.';
}

interface AiModelFieldsProps {
  provider: AiProvider;
  models: ModelSelection;
  onChange: (key: keyof ModelSelection, value: string) => void;
  fastOptions: string[];
  reasoningOptions: string[];
  embeddingOptions: string[];
  installedModels: InstalledModelOption[];
  inheritedLabel: string;
}

function ModelSelect({
  id,
  label,
  value,
  onChange,
  options,
  blankLabel,
  hint,
  full,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: { value: string; label: string; disabled?: boolean }[];
  blankLabel: string;
  hint: string;
  full?: boolean;
}) {
  return (
    <div className={full ? 'form-group form-group--full' : 'form-group'}>
      <label htmlFor={id}>{label}</label>
      <select
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="form-select"
      >
        <option value="">{blankLabel}</option>
        {options.map((option) => (
          <option key={option.value} value={option.value} disabled={option.disabled}>
            {option.label}
          </option>
        ))}
      </select>
      <p className="form-hint">{hint}</p>
    </div>
  );
}

const plain = (options: string[]) => options.map((value) => ({ value, label: value }));

export function AiModelFields({
  provider,
  models,
  onChange,
  fastOptions,
  reasoningOptions,
  embeddingOptions,
  installedModels,
  inheritedLabel,
}: AiModelFieldsProps) {
  const imageOptions = comfyImageOptions(installedModels, models.image).map((name) => {
    const row = installedModels.find((item) => item.name === name);
    const missing = row?.ready === false;
    return {
      value: name,
      disabled: missing,
      label: missing ? `${name} (requires ${row?.required_files?.[0] || 'base'})` : name,
    };
  });

  const agentic = isAgentProvider(provider);

  return (
    <div className="form-grid">
      <ModelSelect
        id="model-fast"
        label="Fast Model"
        value={models.fast}
        onChange={(value) => onChange('fast', value)}
        options={plain(fastOptions)}
        blankLabel="Automatic"
        hint={
          agentic
            ? 'Automatic lets the agent choose; titles, PR subjects and summaries use it too.'
            : 'Short replies, titles and intent classification. Empty picks from the installed models.'
        }
      />
      <ModelSelect
        id="model-reasoning"
        label="Reasoning Model"
        value={models.reasoning}
        onChange={(value) => onChange('reasoning', value)}
        options={plain(reasoningOptions)}
        blankLabel="Automatic"
        hint={
          agentic
            ? 'Harder questions; empty lets the agent choose.'
            : 'Harder questions; empty picks a larger installed model.'
        }
      />
      {embeddingOptions.length > 0 ? (
        <ModelSelect
          id="model-embedding"
          label="Embedding Model"
          value={models.embedding}
          onChange={(value) => onChange('embedding', value)}
          options={plain(embeddingOptions)}
          blankLabel="Automatic"
          hint={embeddingHint(provider, true)}
          full
        />
      ) : (
        <div className="form-group form-group--full">
          <label htmlFor="model-embedding">Embedding Model</label>
          <input
            type="text"
            id="model-embedding"
            value={models.embedding}
            onChange={(event) => onChange('embedding', event.target.value)}
            placeholder={agentic ? 'Server default' : 'text-embedding-3-small'}
            className="form-input"
          />
          <p className="form-hint">{embeddingHint(provider, false)}</p>
        </div>
      )}
      <div className="form-grid form-grid--3">
        <ModelSelect
          id="model-image"
          label="Image Model"
          value={models.image}
          onChange={(value) => onChange('image', value)}
          options={imageOptions}
          blankLabel={inheritedLabel}
          hint="ComfyUI checkpoint for image requests."
        />
        <ModelSelect
          id="model-video"
          label="Video Model"
          value={models.video}
          onChange={(value) => onChange('video', value)}
          options={plain(withCurrent(VIDEO_MODEL_OPTIONS, models.video))}
          blankLabel={inheritedLabel}
          hint="ComfyUI UNET for video requests."
        />
        <ModelSelect
          id="model-audio"
          label="Audio Model"
          value={models.audio}
          onChange={(value) => onChange('audio', value)}
          options={plain(withCurrent(AUDIO_MODEL_OPTIONS, models.audio))}
          blankLabel={inheritedLabel}
          hint="ComfyUI checkpoint for audio requests."
        />
      </div>
    </div>
  );
}

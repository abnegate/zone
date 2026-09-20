import type { ModelCapability } from '../types';
import './Capabilities.css';

const labels: Record<ModelCapability, string> = {
  text: 'Text',
  image_input: 'Image input',
  image_generation: 'Image generation',
  audio: 'Audio',
  audio_input: 'Audio input',
  audio_generation: 'Audio generation',
  video_input: 'Video input',
  video_generation: 'Video generation',
  tools: 'Tools',
  embeddings: 'Embeddings',
  reasoning: 'Reasoning',
};

const coveredBy: Record<string, ModelCapability> = {
  text: 'text',
  tools: 'tools',
  thinking: 'reasoning',
  reasoning: 'reasoning',
  vision: 'image_input',
  embedding: 'embeddings',
  embeddings: 'embeddings',
  audio: 'audio',
};

function sentenceCase(tag: string): string {
  return tag.charAt(0).toUpperCase() + tag.slice(1).toLowerCase();
}

function uncoveredTags(capabilities: ModelCapability[], tags?: string[] | null): string[] {
  const seen = new Set<string>();
  return (tags ?? [])
    .map((tag) => tag.trim().toLowerCase())
    .filter((tag) => {
      if (!tag || seen.has(tag)) return false;
      seen.add(tag);
      const capability = coveredBy[tag];
      return !(capability && capabilities.includes(capability));
    });
}

export default function Capabilities({
  capabilities,
  tags,
}: {
  capabilities?: ModelCapability[] | null;
  tags?: string[] | null;
}) {
  const known = [...new Set(capabilities ?? [])];
  const extra = uncoveredTags(known, tags);
  return (
    <div className="model-capabilities" role="group" aria-label="Model capabilities">
      {known.length === 0 && extra.length === 0 ? (
        <span className="tag">Capabilities unknown</span>
      ) : (
        <>
          {known.map((capability) => (
            <span className="tag" key={capability}>
              {labels[capability]}
            </span>
          ))}
          {extra.map((tag) => (
            <span className="tag" key={tag}>
              {sentenceCase(tag)}
            </span>
          ))}
        </>
      )}
    </div>
  );
}

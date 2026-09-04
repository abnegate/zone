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

export default function Capabilities({
  capabilities,
}: {
  capabilities?: ModelCapability[] | null;
}) {
  return (
    <div className="model-capabilities" role="group" aria-label="Model capabilities">
      {capabilities?.length ? (
        [...new Set(capabilities)].map((capability) => (
          <span className="tag" key={capability}>
            {labels[capability]}
          </span>
        ))
      ) : (
        <span className="tag">Capabilities unknown</span>
      )}
    </div>
  );
}

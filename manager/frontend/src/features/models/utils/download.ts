import type { BrowseModel, BrowseSource } from '../types';
import { defaultDownloadName } from './formatters';

type Download =
  | { name: string; label: 'Install'; reason: null }
  | { name: null; label: 'Remote API'; reason: string };

export function modelDownload(
  model: BrowseModel,
  name: string = defaultDownloadName(model),
  source: BrowseSource = 'all'
): Download {
  const catalog = model.source ?? source;
  if (model.details?.format === 'api') {
    return {
      name: null,
      label: 'Remote API',
      reason: 'This model uses a remote API and cannot be installed through Ollama.',
    };
  }
  if (isComfyFormat(model.details?.format)) {
    return { name, label: 'Install', reason: null };
  }
  const qualified = name.startsWith('hf.co/') || name.startsWith('huggingface.co/');
  return {
    name: catalog === 'huggingface' && !qualified ? `hf.co/${name}` : name,
    label: 'Install',
    reason: null,
  };
}

export function isComfyFormat(format?: string | null): boolean {
  return format === 'lora' || format === 'checkpoint' || format === 'diffusion_model';
}

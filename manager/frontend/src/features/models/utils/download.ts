import type { BrowseModel, BrowseSource } from '../types';
import { defaultDownloadName } from './formatters';

type Download =
  | { name: string; label: 'Install'; reason: null }
  | { name: null; label: 'Remote API' | 'Download unavailable'; reason: string };

export function modelDownload(
  model: BrowseModel,
  name: string = defaultDownloadName(model),
  source: BrowseSource = 'all'
): Download {
  const catalog = model.source ?? source;
  if (catalog === 'openrouter' || model.details?.format === 'api') {
    return {
      name: null,
      label: 'Remote API',
      reason: 'This model uses a remote API and cannot be installed through Ollama.',
    };
  }
  if (catalog === 'gpt4all') {
    return {
      name: null,
      label: 'Download unavailable',
      reason:
        'GPT4All downloads cannot be installed through Ollama. Find an Ollama or HuggingFace GGUF version.',
    };
  }
  const qualified = name.startsWith('hf.co/') || name.startsWith('huggingface.co/');
  return {
    name: catalog === 'huggingface' && !qualified ? `hf.co/${name}` : name,
    label: 'Install',
    reason: null,
  };
}

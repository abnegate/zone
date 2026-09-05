import type { BrowseSource } from '../types';

export function modelSourceUrl(
  name: string,
  url?: string | null,
  source?: BrowseSource | null
): string | null {
  if (url) return url;
  const trimmed = name.trim();
  if (!trimmed) return null;

  const huggingface = trimmed.match(/^(?:hf\.co|huggingface\.co)\/([^:]+)/i);
  if (huggingface) {
    return `https://huggingface.co/${huggingface[1]}`;
  }

  const identifier = trimmed.split(':')[0];
  if (!identifier) return null;

  if (source === 'huggingface') {
    return `https://huggingface.co/${identifier}`;
  }

  return identifier.includes('/')
    ? `https://ollama.com/${identifier}`
    : `https://ollama.com/library/${identifier}`;
}

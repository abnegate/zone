import type { BrowseSource } from '../types';

const LABELS: Record<BrowseSource, string> = {
  all: 'All',
  ollama: 'Ollama',
  huggingface: 'Hugging Face',
};

export function sourceLabel(source: BrowseSource | null | undefined): string {
  return source ? LABELS[source] : '';
}

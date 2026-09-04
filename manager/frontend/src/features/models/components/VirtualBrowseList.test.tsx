import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { BrowseModel } from '../types';
import VirtualBrowseList from './VirtualBrowseList';

const mockModels: BrowseModel[] = [
  {
    name: 'llama2:7b',
    size: 3800000000,
    description: 'A compact Llama 2 chat model.',
    downloads: 28700,
    capabilities: ['text', 'image_input', 'audio_input', 'reasoning', 'tools', 'image_generation'],
    details: {
      family: 'llama',
      parameter_size: '7B',
      quantization_level: 'Q4_0',
    },
  },
  {
    name: 'mistral:latest',
    size: 4100000000,
    details: {
      family: 'mistral',
      parameter_size: '7B',
    },
  },
];

describe('VirtualBrowseList', () => {
  const onItemClick = mock();
  const onInstall = mock();
  const onLoadMore = mock();

  beforeEach(() => {
    mock.clearAllMocks();
  });

  it('shows empty state when no models', () => {
    render(
      <VirtualBrowseList
        models={[]}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('No models found')).toBeInTheDocument();
  });

  it('renders model names', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('llama2:7b')).toBeInTheDocument();
    expect(screen.getByText('mistral:latest')).toBeInTheDocument();
  });

  it('renders model sizes', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('3.5 GB')).toBeInTheDocument();
    expect(screen.getByText('3.8 GB')).toBeInTheDocument();
  });

  it('renders parameter size, description, and all capabilities', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getAllByText('7B').length).toBeGreaterThan(0);
    expect(screen.getByText('Q4_0')).toBeInTheDocument();
    expect(screen.getByText('llama')).toBeInTheDocument();
    expect(screen.getByText('A compact Llama 2 chat model.')).toBeInTheDocument();
    expect(screen.getByText('Text')).toBeInTheDocument();
    expect(screen.getByText('Image input')).toBeInTheDocument();
    expect(screen.getByText('Image generation')).toBeInTheDocument();
    expect(screen.getByText('Reasoning')).toBeInTheDocument();
    expect(screen.getByText('Audio input')).toBeInTheDocument();
    expect(screen.getByText('Tools')).toBeInTheDocument();
    expect(screen.getByText('28.7K downloads')).toBeInTheDocument();
  });

  it('renders zero downloads with source-aware labels', () => {
    render(
      <VirtualBrowseList
        models={[
          { name: 'ollama-zero', downloads: 0, source: 'ollama' },
          { name: 'huggingface-zero', downloads: 0, source: 'huggingface' },
        ]}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('0 pulls')).toBeInTheDocument();
    expect(screen.getByText('0 downloads')).toBeInTheDocument();
    expect(document.querySelectorAll('.browse-downloads')).toHaveLength(2);
  });

  it('prefers display name when present', () => {
    render(
      <VirtualBrowseList
        models={[
          {
            name: 'anthropic/claude-sonnet-4',
            display_name: 'Anthropic: Claude Sonnet 4',
            description: 'A balanced coding model.',
          },
        ]}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('Anthropic: Claude Sonnet 4')).toBeInTheDocument();
  });

  it('calls onItemClick when model clicked', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    fireEvent.click(screen.getByText('llama2:7b').closest('.browse-item')!);

    expect(onItemClick).toHaveBeenCalledWith(mockModels[0]);
  });

  it('calls onInstall when install button clicked', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    const installButtons = screen.getAllByRole('button', { name: 'Install' });
    fireEvent.click(installButtons[0]);

    expect(onInstall).toHaveBeenCalledWith(mockModels[0]);
    expect(onItemClick).not.toHaveBeenCalled();
  });

  for (const source of ['openrouter', 'gpt4all'] as const) {
    it(`explains why ${source} models cannot be installed through Ollama`, () => {
      const model: BrowseModel = { name: 'qwen/qwen3.8-27b', source };
      render(
        <VirtualBrowseList
          models={[model]}
          onItemClick={onItemClick}
          onInstall={onInstall}
          hasMore={false}
          loadingMore={false}
          onLoadMore={onLoadMore}
        />
      );

      expect(screen.queryByRole('button', { name: 'Install' })).not.toBeInTheDocument();
      expect(screen.getByText(/cannot be installed through Ollama/)).toBeInTheDocument();
      const action = screen.getByRole('button', {
        name: source === 'openrouter' ? 'Remote API' : 'Download unavailable',
      });
      expect(action).toBeDisabled();
      fireEvent.click(action);
      expect(onInstall).not.toHaveBeenCalled();

      fireEvent.click(screen.getByText(model.name));
      expect(onItemClick).toHaveBeenCalledWith(model);
    });
  }

  it('shows loading indicator when loadingMore', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={true}
        loadingMore={true}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('Loading more...')).toBeInTheDocument();
  });

  it('handles keyboard navigation', () => {
    render(
      <VirtualBrowseList
        models={mockModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    const firstItem = screen.getByText('llama2:7b').closest('.browse-item')!;
    fireEvent.keyDown(firstItem, { key: 'Enter' });

    expect(onItemClick).toHaveBeenCalledWith(mockModels[0]);
  });

  it('renders model without size', () => {
    const noSizeModels: BrowseModel[] = [
      {
        name: 'no-size-model',
        details: {
          family: 'test',
        },
      },
    ];

    render(
      <VirtualBrowseList
        models={noSizeModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('no-size-model')).toBeInTheDocument();
    expect(document.querySelectorAll('.browse-size').length).toBe(0);
  });

  it('renders model without details', () => {
    const noDetailsModels: BrowseModel[] = [
      {
        name: 'no-details-model',
        size: 1000000000,
        use_cases: ['Tools'],
      },
    ];

    render(
      <VirtualBrowseList
        models={noDetailsModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('no-details-model')).toBeInTheDocument();
    expect(screen.getByText('Capabilities unknown')).toBeInTheDocument();
    expect(screen.queryByText('Tools')).not.toBeInTheDocument();
    expect(document.querySelectorAll('.browse-tags').length).toBe(0);
  });
});

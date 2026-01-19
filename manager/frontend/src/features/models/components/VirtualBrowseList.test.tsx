import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, mock } from 'bun:test';
import type { BrowseModel } from '../types';
import VirtualBrowseList from './VirtualBrowseList';

const mockModels: BrowseModel[] = [
  {
    name: 'llama2:7b',
    size: 3800000000,
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

  it('renders model details as tags', () => {
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

    expect(screen.getByText('llama')).toBeInTheDocument();
    expect(screen.getByText('mistral')).toBeInTheDocument();
    expect(screen.getAllByText('7B').length).toBeGreaterThan(0);
    expect(screen.getByText('Q4_0')).toBeInTheDocument();
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
    expect(document.querySelectorAll('.browse-tags').length).toBe(0);
  });
});

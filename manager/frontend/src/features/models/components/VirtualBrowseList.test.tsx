import { fireEvent, render, screen } from '@testing-library/react';
import type { BrowseModel } from '../types';
import VirtualBrowseList from './VirtualBrowseList';

const mockModels: BrowseModel[] = [
  {
    id: 'model-1',
    name: 'llama2',
    description: 'A large language model',
    downloads: 1500000,
    tags: ['llm', 'text-generation', 'chat'],
  },
  {
    id: 'model-2',
    name: 'mistral',
    description: 'Fast and efficient model',
    downloads: 750000,
    tags: ['llm', 'fast'],
  },
];

describe('VirtualBrowseList', () => {
  const onItemClick = jest.fn();
  const onInstall = jest.fn();
  const onLoadMore = jest.fn();

  beforeEach(() => {
    jest.clearAllMocks();
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

    expect(screen.getByText('llama2')).toBeInTheDocument();
    expect(screen.getByText('mistral')).toBeInTheDocument();
  });

  it('renders model descriptions', () => {
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

    expect(screen.getByText('A large language model')).toBeInTheDocument();
    expect(screen.getByText('Fast and efficient model')).toBeInTheDocument();
  });

  it('formats downloads correctly', () => {
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

    expect(screen.getByText('1.5M downloads')).toBeInTheDocument();
    expect(screen.getByText('750.0K downloads')).toBeInTheDocument();
  });

  it('renders tags', () => {
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

    // Check that tags are present (there may be multiple with same text)
    expect(screen.getAllByText('llm').length).toBeGreaterThan(0);
    expect(screen.getByText('text-generation')).toBeInTheDocument();
    expect(screen.getByText('chat')).toBeInTheDocument();
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

    fireEvent.click(screen.getByText('llama2').closest('.browse-item')!);

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

    const firstItem = screen.getByText('llama2').closest('.browse-item')!;
    fireEvent.keyDown(firstItem, { key: 'Enter' });

    expect(onItemClick).toHaveBeenCalledWith(mockModels[0]);
  });

  it('formats downloads less than 1000 correctly', () => {
    const smallDownloadModels: BrowseModel[] = [
      {
        id: 'model-3',
        name: 'small-model',
        description: 'A model with few downloads',
        downloads: 42,
        tags: [],
      },
    ];

    render(
      <VirtualBrowseList
        models={smallDownloadModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('42 downloads')).toBeInTheDocument();
  });

  it('renders model without description', () => {
    const noDescModels: BrowseModel[] = [
      {
        id: 'model-4',
        name: 'no-desc-model',
        description: '',
        downloads: 1000,
        tags: ['test'],
      },
    ];

    render(
      <VirtualBrowseList
        models={noDescModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('no-desc-model')).toBeInTheDocument();
    expect(document.querySelectorAll('.browse-description').length).toBe(0);
  });

  it('renders model without tags', () => {
    const noTagsModels: BrowseModel[] = [
      {
        id: 'model-5',
        name: 'no-tags-model',
        description: 'A model',
        downloads: 500,
        tags: [],
      },
    ];

    render(
      <VirtualBrowseList
        models={noTagsModels}
        onItemClick={onItemClick}
        onInstall={onInstall}
        hasMore={false}
        loadingMore={false}
        onLoadMore={onLoadMore}
      />
    );

    expect(screen.getByText('no-tags-model')).toBeInTheDocument();
    expect(document.querySelectorAll('.browse-tags').length).toBe(0);
  });
});

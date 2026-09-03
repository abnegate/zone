import React, { useCallback, useEffect, useRef } from 'react';
import { VariableSizeList as List, type ListChildComponentProps } from 'react-window';
import type { BrowseModel } from '../types';
import { formatBytes, formatContextLength, formatNumber } from '../utils';
import './VirtualBrowseList.css';

interface VirtualBrowseListProps {
  models: BrowseModel[];
  onItemClick: (model: BrowseModel) => void;
  onInstall: (model: BrowseModel) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}

const LOADING_ROW_HEIGHT = 56;

function browseItemHeight(model: BrowseModel): number {
  let height = 76;
  if (model.description) height += 40;
  if (hasUseCases(model) || model.downloads) height += 28;
  return height;
}

function hasUseCases(model: BrowseModel): boolean {
  return Boolean(model.use_cases && model.use_cases.length > 0);
}

function specParts(model: BrowseModel): string[] {
  const parts: string[] = [];
  if (model.details?.parameter_size) parts.push(model.details.parameter_size);
  if (model.size) parts.push(formatBytes(model.size));
  if (model.details?.quantization_level) parts.push(model.details.quantization_level);
  if (model.details?.context_length) {
    parts.push(`${formatContextLength(model.details.context_length)} ctx`);
  }
  if (model.details?.family) parts.push(model.details.family);
  return parts;
}

export default function VirtualBrowseList({
  models,
  onItemClick,
  onInstall,
  hasMore,
  loadingMore,
  onLoadMore,
}: VirtualBrowseListProps) {
  const listRef = useRef<List>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleItemsRendered = useCallback(
    ({ visibleStopIndex }: { visibleStopIndex: number }) => {
      if (hasMore && !loadingMore && visibleStopIndex >= models.length - 5) {
        onLoadMore();
      }
    },
    [hasMore, loadingMore, models.length, onLoadMore]
  );

  const itemCount = models.length + (hasMore ? 1 : 0);

  const getItemSize = useCallback(
    (index: number) => {
      if (index >= models.length) return LOADING_ROW_HEIGHT;
      return browseItemHeight(models[index]);
    },
    [models]
  );

  // Reset cached row heights whenever the catalogue contents change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: models identity is the catalogue snapshot
  useEffect(() => {
    listRef.current?.resetAfterIndex(0);
  }, [models]);

  const Row = useCallback(
    ({ index, style }: ListChildComponentProps) => {
      if (index >= models.length) {
        return (
          <div style={style} className="virtual-browse-loading">
            <span className="spinner" /> Loading more...
          </div>
        );
      }

      const model = models[index];
      const specs = specParts(model);
      const useCases = model.use_cases ?? [];
      const title = model.display_name || model.name;

      return (
        <div style={style} className="virtual-browse-item-wrapper">
          <div
            className="browse-item"
            onClick={() => onItemClick(model)}
            onKeyDown={(e) => e.key === 'Enter' && onItemClick(model)}
            role="button"
            tabIndex={0}
          >
            <div className="browse-info">
              <div className="browse-header">
                <span className="browse-name">{title}</span>
                {model.source && (
                  <span className={`browse-source browse-source-${model.source}`}>
                    {model.source}
                  </span>
                )}
              </div>
              {specs.length > 0 && (
                <div className="browse-specs">
                  {specs.map((part) => (
                    <span key={part} className="browse-spec">
                      {part}
                    </span>
                  ))}
                </div>
              )}
              {model.description && <p className="browse-description">{model.description}</p>}
              {(useCases.length > 0 || model.downloads) && (
                <div className="browse-tags">
                  {useCases.slice(0, 4).map((useCase) => (
                    <span key={useCase} className="tag">
                      {useCase}
                    </span>
                  ))}
                  {model.downloads ? (
                    <span className="browse-downloads">
                      {formatNumber(model.downloads)}
                      {model.source === 'ollama' ? ' pulls' : ' downloads'}
                    </span>
                  ) : null}
                </div>
              )}
            </div>
            <button
              className="btn btn-primary btn-small"
              onClick={(e) => {
                e.stopPropagation();
                onInstall(model);
              }}
              type="button"
            >
              Install
            </button>
          </div>
        </div>
      );
    },
    [models, onItemClick, onInstall]
  );

  const [containerHeight, setContainerHeight] = React.useState(400);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateHeight = () => {
      const next = el.clientHeight || el.parentElement?.clientHeight || 400;
      setContainerHeight(Math.max(next, 200));
    };

    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(el);
    if (el.parentElement) observer.observe(el.parentElement);
    return () => observer.disconnect();
  }, []);

  if (models.length === 0 && !loadingMore) {
    return <div className="empty-placeholder">No models found</div>;
  }

  return (
    <div ref={containerRef} className="virtual-browse-container">
      <List
        ref={listRef}
        height={containerHeight}
        itemCount={itemCount}
        itemSize={getItemSize}
        width="100%"
        overscanCount={8}
        onItemsRendered={handleItemsRendered}
        className="virtual-browse-list"
      >
        {Row}
      </List>
    </div>
  );
}

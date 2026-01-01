import React, { useCallback, useEffect, useRef } from 'react';
import { FixedSizeList as List, type ListChildComponentProps } from 'react-window';
import type { BrowseModel } from '../types';
import './VirtualBrowseList.css';

interface VirtualBrowseListProps {
  models: BrowseModel[];
  onItemClick: (model: BrowseModel) => void;
  onInstall: (model: BrowseModel) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}

function formatNumber(num: number): string {
  if (num >= 1000000) return `${(num / 1000000).toFixed(1)}M`;
  if (num >= 1000) return `${(num / 1000).toFixed(1)}K`;
  return num.toString();
}

const ITEM_HEIGHT = 120;

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

  // Load more when scrolling near the end
  const handleItemsRendered = useCallback(
    ({ visibleStopIndex }: { visibleStopIndex: number }) => {
      if (hasMore && !loadingMore && visibleStopIndex >= models.length - 5) {
        onLoadMore();
      }
    },
    [hasMore, loadingMore, models.length, onLoadMore]
  );

  // Get total item count including loading indicator
  const itemCount = models.length + (hasMore ? 1 : 0);

  const Row = useCallback(
    ({ index, style }: ListChildComponentProps) => {
      // Loading row
      if (index >= models.length) {
        return (
          <div style={style} className="virtual-browse-loading">
            <span className="spinner" /> Loading more...
          </div>
        );
      }

      const model = models[index];

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
                <span className="browse-name">{model.name}</span>
                <span className="browse-downloads">{formatNumber(model.downloads)} downloads</span>
              </div>
              {model.description && <p className="browse-description">{model.description}</p>}
              {model.tags.length > 0 && (
                <div className="browse-tags">
                  {model.tags.slice(0, 5).map((tag) => (
                    <span key={tag} className="tag">
                      {tag}
                    </span>
                  ))}
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

  // Determine container height
  const [containerHeight, setContainerHeight] = React.useState(400);

  useEffect(() => {
    const updateHeight = () => {
      if (containerRef.current) {
        // Use parent height or fallback to 400
        const parentHeight = containerRef.current.parentElement?.clientHeight;
        setContainerHeight(parentHeight ? Math.min(parentHeight, 400) : 400);
      }
    };

    updateHeight();
    window.addEventListener('resize', updateHeight);
    return () => window.removeEventListener('resize', updateHeight);
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
        itemSize={ITEM_HEIGHT}
        width="100%"
        onItemsRendered={handleItemsRendered}
        className="virtual-browse-list"
      >
        {Row}
      </List>
    </div>
  );
}

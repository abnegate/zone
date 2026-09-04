import React, { useCallback, useEffect, useRef } from 'react';
import { List, type RowComponentProps, useDynamicRowHeight } from 'react-window';
import type { BrowseModel } from '../types';
import { formatBytes, formatContextLength, formatNumber } from '../utils';
import Capabilities from './Capabilities';
import './VirtualBrowseList.css';

interface VirtualBrowseListProps {
  models: BrowseModel[];
  onItemClick: (model: BrowseModel) => void;
  onInstall: (model: BrowseModel) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}

type BrowseRowProps = {
  models: BrowseModel[];
  onItemClick: (model: BrowseModel) => void;
  onInstall: (model: BrowseModel) => void;
};

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

function BrowseRow({
  index,
  style,
  models,
  onItemClick,
  onInstall,
}: RowComponentProps<BrowseRowProps>) {
  if (index >= models.length) {
    return (
      <div style={style} className="virtual-browse-loading">
        <span className="spinner" /> Loading more...
      </div>
    );
  }

  const model = models[index];
  const specs = specParts(model);
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
              <span className={`browse-source browse-source-${model.source}`}>{model.source}</span>
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
          <Capabilities capabilities={model.capabilities} />
          {model.downloads ? (
            <span className="browse-downloads">
              {formatNumber(model.downloads)}
              {model.source === 'ollama' ? ' pulls' : ' downloads'}
            </span>
          ) : null}
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
}

export default function VirtualBrowseList({
  models,
  onItemClick,
  onInstall,
  hasMore,
  loadingMore,
  onLoadMore,
}: VirtualBrowseListProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const handleRowsRendered = useCallback(
    ({ stopIndex }: { stopIndex: number }) => {
      if (hasMore && !loadingMore && stopIndex >= models.length - 5) {
        onLoadMore();
      }
    },
    [hasMore, loadingMore, models.length, onLoadMore]
  );

  const itemCount = models.length + (hasMore ? 1 : 0);

  const rowHeight = useDynamicRowHeight({ defaultRowHeight: 160 });

  const rowProps = React.useMemo<BrowseRowProps>(
    () => ({
      models,
      onItemClick,
      onInstall,
    }),
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
        rowCount={itemCount}
        rowHeight={rowHeight}
        rowComponent={BrowseRow}
        rowProps={rowProps}
        overscanCount={8}
        onRowsRendered={handleRowsRendered}
        className="virtual-browse-list"
        style={{ height: containerHeight, width: '100%' }}
      />
    </div>
  );
}

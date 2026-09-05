import type { BrowseModel, ModelSizeOption } from '../types';
import { downloadOptionRows, formatBytes, modelDownload } from '../utils';
import './DownloadOptions.css';

interface DownloadOptionsProps {
  model: BrowseModel;
  options: ModelSizeOption[];
  pulling?: boolean;
  onInstall: (name: string) => void;
}

export default function DownloadOptions({
  model,
  options,
  pulling = false,
  onInstall,
}: DownloadOptionsProps) {
  if (options.length < 2) return null;

  const rows = downloadOptionRows(options);
  const quantized = rows.some((row) => row.group?.endsWith('-bit'));

  return (
    <div className="details-downloads">
      <span className="details-label">{quantized ? 'GGUF downloads' : 'Download options'}</span>
      <p className="help-text">
        {quantized
          ? 'Each quantization is a separate install.'
          : 'This model is published in more than one size.'}
      </p>
      <div className="details-download-list" role="list">
        {rows.map(({ heading, group, option }) => {
          const download = modelDownload(model, option.name);
          const sizeLabel = option.size != null ? formatBytes(option.size) : null;
          const accessible = ['Install', group, option.label, sizeLabel].filter(Boolean).join(' ');

          return (
            <div key={option.name} className="details-download-row" role="listitem">
              <span className="details-download-group">{heading ?? ''}</span>
              <button
                type="button"
                className="details-download-chip"
                aria-label={accessible}
                title={download.name ?? option.name}
                disabled={pulling || download.name === null}
                onClick={() => download.name && onInstall(option.name)}
              >
                <span>{option.label}</span>
                {sizeLabel && (
                  <>
                    <span className="details-download-pipe" aria-hidden="true">
                      |
                    </span>
                    <span>{sizeLabel}</span>
                  </>
                )}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

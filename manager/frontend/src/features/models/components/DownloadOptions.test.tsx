import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { BrowseModel } from '../types';
import DownloadOptions from './DownloadOptions';

const model: BrowseModel = {
  name: 'TheBloke/Mistral-7B-Instruct-v0.2-GGUF',
  source: 'huggingface',
};

describe('DownloadOptions', () => {
  it('renders each GGUF quantization as its own install', () => {
    const onInstall = mock();
    render(
      <DownloadOptions
        model={model}
        onInstall={onInstall}
        options={[
          { name: `${model.name}:Q4_0`, label: 'Q4_0', size: 4_108_917_024 },
          { name: `${model.name}:Q5_K_M`, label: 'Q5_K_M', size: 5_131_409_696 },
          { name: `${model.name}:Q8_0`, label: 'Q8_0', size: 7_695_857_952 },
        ]}
      />
    );

    expect(screen.getByText('GGUF downloads')).toBeInTheDocument();
    expect(screen.getByText('4-bit')).toBeInTheDocument();
    expect(screen.getByText('5-bit')).toBeInTheDocument();
    expect(screen.getByText('8-bit')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Install 4-bit Q4_0/ }));
    expect(onInstall).toHaveBeenCalledWith(`${model.name}:Q4_0`);
  });

  it('does not render a single download as a list', () => {
    render(
      <DownloadOptions
        model={model}
        onInstall={mock()}
        options={[{ name: `${model.name}:Q4_0`, label: 'Q4_0' }]}
      />
    );
    expect(screen.queryByText('GGUF downloads')).not.toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});

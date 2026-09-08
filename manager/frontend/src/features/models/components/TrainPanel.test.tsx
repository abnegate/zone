import { afterAll, beforeAll, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';

const mockTrainBases = mock(() =>
  Promise.resolve([
    { id: 'qwen-image-edit', label: 'Qwen Image Edit', edit: true },
    { id: 'flux-schnell', label: 'FLUX.1 Schnell', edit: false },
  ])
);

mock.module('../../../api/models', () => ({
  modelsApi: {
    trainBases: mockTrainBases,
    captions: mock(),
    train: mock(),
  },
}));

let TrainPanel: typeof import('./TrainPanel').default;

beforeAll(async () => {
  TrainPanel = (await import('./TrainPanel')).default;
});

afterAll(() => {
  mock.restore();
});

describe('TrainPanel', () => {
  it('gives every field its own labelled control', async () => {
    render(<TrainPanel onTrained={mock()} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Name')).toBeInTheDocument();
    });
    for (const label of ['Name', 'Trigger word', 'Images']) {
      expect(screen.getByLabelText(label).tagName).toBe('INPUT');
    }
    expect(screen.getByLabelText('Images').getAttribute('type')).toBe('file');
    expect(screen.getByLabelText('Base').getAttribute('role')).toBe('combobox');
  });

  it('keeps the first base selected once the bases load', async () => {
    render(<TrainPanel onTrained={mock()} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    });
    expect(screen.getByLabelText('Before images (edit bases)')).toBeInTheDocument();
  });
});

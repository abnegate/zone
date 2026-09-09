import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

const mockTrainBases = mock(() =>
  Promise.resolve([
    { id: 'qwen-image-edit', label: 'Qwen Image Edit', edit: true },
    { id: 'flux-schnell', label: 'FLUX.1 Schnell', edit: false },
  ])
);
const mockCaptions = mock(() => Promise.resolve({ captions: ['generated caption'] }));
const mockTrain = mock(() =>
  Promise.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] })
);

mock.module('../../../api/models', () => ({
  modelsApi: {
    trainBases: mockTrainBases,
    captions: mockCaptions,
    train: mockTrain,
  },
}));

let TrainPanel: typeof import('./TrainPanel').default;

beforeAll(async () => {
  TrainPanel = (await import('./TrainPanel')).default;
});

beforeEach(() => {
  mockCaptions.mockClear();
  mockTrain.mockClear();
  mockTrain.mockImplementation(() =>
    Promise.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] })
  );
});

afterAll(() => {
  mock.restore();
});

function file(name: string, contents: string): File {
  return new File([contents], name, { type: 'image/png' });
}

async function addTargets(...files: File[]): Promise<void> {
  fireEvent.change(screen.getByLabelText('Target images'), {
    target: { files },
  });
  await waitFor(() => {
    const pairs = screen.getAllByRole('group', { name: /target pair/i });
    expect(pairs).toHaveLength(files.length);
    for (const pair of pairs) expect(pair).toHaveAttribute('aria-busy', 'false');
  });
}

async function addReference(label: string, reference: File): Promise<void> {
  const input = screen.getByLabelText(label);
  fireEvent.change(input, {
    target: { files: [reference] },
  });
  await waitFor(() => {
    expect(screen.getByText(`Reference: ${reference.name}`)).toBeInTheDocument();
    expect(input.closest('fieldset')).toHaveAttribute('aria-busy', 'false');
  });
}

async function selectBase(label: string): Promise<void> {
  fireEvent.click(screen.getByLabelText('Base'));
  fireEvent.click(await screen.findByRole('option', { name: label }));
  await waitFor(() => {
    expect(screen.getByLabelText('Base')).toHaveTextContent(label);
  });
}

function fillIdentity(): void {
  fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'zoneface' } });
  fireEvent.change(screen.getByLabelText('Trigger word'), { target: { value: 'zne person' } });
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve = (_value: T): void => {};
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function deferFileReads(): {
  reads: Array<{ file: File; complete: (encoded: string) => void }>;
  restore: () => void;
} {
  const OriginalFileReader = globalThis.FileReader;
  const reads: Array<{ file: File; complete: (encoded: string) => void }> = [];

  class DeferredFileReader {
    result: string | ArrayBuffer | null = null;
    error: DOMException | null = null;
    onload: FileReader['onload'] = null;
    onerror: FileReader['onerror'] = null;

    readAsDataURL(file: Blob): void {
      reads.push({
        file: file as File,
        complete: (encoded) => {
          this.result = `data:image/png;base64,${encoded}`;
          this.onload?.call(this as unknown as FileReader, {} as ProgressEvent<FileReader>);
        },
      });
    }
  }

  globalThis.FileReader = DeferredFileReader as unknown as typeof FileReader;
  return {
    reads,
    restore: () => {
      globalThis.FileReader = OriginalFileReader;
    },
  };
}

describe('TrainPanel', () => {
  it('gives every field its own labelled control', async () => {
    render(<TrainPanel onTrained={mock()} />);

    await waitFor(() => {
      expect(screen.getByLabelText('Name')).toBeInTheDocument();
    });
    for (const label of ['Name', 'Trigger word', 'Target images']) {
      expect(screen.getByLabelText(label).tagName).toBe('INPUT');
    }
    expect(screen.getByLabelText('Target images').getAttribute('type')).toBe('file');
    expect(screen.getByLabelText('Base').getAttribute('role')).toBe('combobox');
  });

  it('requires one reference and one instruction for every Qwen target', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'zone-edit' } });
    expect(screen.getByLabelText('Trigger word')).not.toHaveAttribute('required');
    await addTargets(file('after-one.png', 'after one'), file('after-two.png', 'after two'));

    const firstReference = screen.getByLabelText('Reference image for target 1: after-one.png');
    const firstInstruction = screen.getByLabelText('Instruction for target 1: after-one.png');
    const secondInstruction = screen.getByLabelText('Instruction for target 2: after-two.png');
    const submit = screen.getByRole('button', { name: 'Train' });

    expect(submit).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Auto-caption images' })).toBeNull();
    expect(firstReference).toHaveAttribute('aria-describedby');
    expect(firstInstruction).toHaveAttribute('aria-describedby');
    expect(
      document.getElementById(firstReference.getAttribute('aria-describedby') ?? '')
    ).toHaveTextContent('Choose one reference image for this target.');
    expect(
      document.getElementById(firstInstruction.getAttribute('aria-describedby') ?? '')
    ).toHaveTextContent('Describe the edit that turns the reference into this target.');

    await addReference(
      'Reference image for target 1: after-one.png',
      file('before-one.png', 'before one')
    );
    fireEvent.change(firstInstruction, { target: { value: 'add a red coat' } });
    expect(submit).toBeDisabled();

    await addReference(
      'Reference image for target 2: after-two.png',
      file('before-two.png', 'before two')
    );
    fireEvent.change(secondInstruction, { target: { value: 'move the subject outside' } });
    await waitFor(() => expect(submit).toBeEnabled());

    fireEvent.click(submit);
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    const request = mockTrain.mock.calls[0]?.[0];
    expect(request?.trigger).toBeUndefined();
    expect(request?.images).toHaveLength(2);
    expect(request?.images[0]).toMatchObject({
      filename: 'after-one.png',
      caption: 'add a red coat',
    });
    expect(request?.images[0]?.before_base64).toBeTruthy();
    expect(request?.images[1]).toMatchObject({
      filename: 'after-two.png',
      caption: 'move the subject outside',
    });
    expect(request?.images[1]?.before_base64).toBeTruthy();
  });

  it('clears edit-only state when switching away and revalidates when switching back', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await addTargets(file('after.png', 'after'));
    await addReference('Reference image for target 1: after.png', file('before.png', 'before'));
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after.png'), {
      target: { value: 'turn the shirt blue' },
    });

    await selectBase('FLUX.1 Schnell');
    expect(screen.queryByLabelText('Reference image for target 1: after.png')).toBeNull();
    expect(screen.queryByLabelText('Instruction for target 1: after.png')).toBeNull();
    expect(screen.getByLabelText('Caption for after.png')).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Auto-caption images' })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('Caption for after.png'), {
      target: { value: 'a portrait in cool light' },
    });

    await selectBase('Qwen Image Edit');
    const reference = screen.getByLabelText('Reference image for target 1: after.png');
    const instruction = screen.getByLabelText('Instruction for target 1: after.png');
    expect(reference).toHaveValue('');
    expect(instruction).toHaveValue('');
    expect(screen.getByRole('button', { name: 'Train' })).toBeDisabled();
    expect(
      screen.getByText('1 target pair still needs a reference image and instruction.')
    ).toBeInTheDocument();
  });

  it('keeps each pair together when targets are added, removed, and reordered', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fillIdentity();
    await addTargets(file('after-a.png', 'after a'), file('after-b.png', 'after b'));
    await addReference(
      'Reference image for target 1: after-a.png',
      file('before-a.png', 'before a')
    );
    await addReference(
      'Reference image for target 2: after-b.png',
      file('before-b.png', 'before b')
    );
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after-a.png'), {
      target: { value: 'instruction a' },
    });
    fireEvent.change(screen.getByLabelText('Instruction for target 2: after-b.png'), {
      target: { value: 'instruction b' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Move target 2: after-b.png up' }));
    expect(screen.getByLabelText('Instruction for target 1: after-b.png')).toHaveValue(
      'instruction b'
    );
    expect(screen.getByText('Reference: before-b.png')).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Target images'), {
      target: { files: [file('after-c.png', 'after c')] },
    });
    await screen.findByLabelText('Instruction for target 3: after-c.png');
    await addReference(
      'Reference image for target 3: after-c.png',
      file('before-c.png', 'before c')
    );
    fireEvent.change(screen.getByLabelText('Instruction for target 3: after-c.png'), {
      target: { value: 'instruction c' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Remove target 2: after-a.png' }));
    expect(screen.getByLabelText('Instruction for target 1: after-b.png')).toHaveValue(
      'instruction b'
    );
    expect(screen.getByLabelText('Instruction for target 2: after-c.png')).toHaveValue(
      'instruction c'
    );
    expect(screen.getByText('Reference: before-b.png')).toBeInTheDocument();
    expect(screen.getByText('Reference: before-c.png')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    const request = mockTrain.mock.calls[0]?.[0];
    expect(request?.images.map((image) => [image.filename, image.caption])).toEqual([
      ['after-b.png', 'instruction b'],
      ['after-c.png', 'instruction c'],
    ]);
  });

  it('announces pair readiness and focuses the first incomplete control', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await addTargets(file('after-one.png', 'one'), file('after-two.png', 'two'));

    const status = screen.getByRole('status', { name: 'Training pair readiness' });
    expect(status).toHaveAttribute('aria-live', 'polite');
    expect(status).toHaveTextContent(
      '2 target pairs still need a reference image and instruction.'
    );

    const form = screen.getByRole('button', { name: 'Train' }).closest('form');
    expect(form).not.toBeNull();
    fireEvent.submit(form as HTMLFormElement);
    await waitFor(() => {
      expect(document.activeElement).toBe(
        screen.getByLabelText('Reference image for target 1: after-one.png')
      );
    });
    expect(mockTrain).not.toHaveBeenCalled();
  });

  it('shows measured Qwen quality without FLUX health bands', async () => {
    mockTrain.mockImplementationOnce(() =>
      Promise.resolve({
        filename: 'zoneface.safetensors',
        quality: {
          improvement: 0.3472,
          checkpoint: 'step400',
          measured: true,
          calibration: 'uncalibrated' as const,
        },
        dataset: [],
      })
    );
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fillIdentity();
    await addTargets(file('after.png', 'after'));
    await addReference('Reference image for target 1: after.png', file('before.png', 'before'));
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after.png'), {
      target: { value: 'change the background to a studio' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));

    await screen.findByText('Measured, not calibrated');
    expect(screen.getByText('35% better than base')).toBeInTheDocument();
    expect(
      screen.getByText(/Qwen Image Edit health bands are not calibrated yet/)
    ).toBeInTheDocument();
    expect(screen.queryByText('Weak')).toBeNull();
    expect(screen.queryByText('Healthy')).toBeNull();
    expect(screen.queryByText('Strong')).toBeNull();
  });

  it('reports every image repair attempt after the final screen', async () => {
    mockTrain.mockImplementationOnce(() =>
      Promise.resolve({
        filename: 'zoneface.safetensors',
        quality: null,
        dataset: [],
        screening: {
          kept: 3,
          dropped: [],
          attempted: [
            { filename: 'small.png', reason: 'small' as const, outcome: 'used' as const },
            {
              filename: 'blurred.png',
              reason: 'blurred' as const,
              outcome: 'still_rejected' as const,
            },
            { filename: 'broken.png', reason: 'small' as const, outcome: 'failed' as const },
          ],
        },
      })
    );
    render(<TrainPanel onTrained={mock()} />);
    await selectBase('FLUX.1 Schnell');
    fillIdentity();
    await addTargets(file('target.png', 'target'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));

    await screen.findByText('Image repairs');
    expect(
      screen.getByText('Zone tried to improve these images before the final screen:')
    ).toBeInTheDocument();
    expect(
      screen.getByText('It passed the final screen, and the improved copy was used.')
    ).toBeInTheDocument();
    expect(
      screen.getByText('It did not pass the final screen, so it was not used.')
    ).toBeInTheDocument();
    expect(
      screen.getByText('Zone could not create an improved copy, so it was not used.')
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        'Your originals are untouched. Zone only uses repaired copies inside this training run.'
      )
    ).toBeInTheDocument();
  });

  it('merges deferred captions by stable target key without overwriting later edits', async () => {
    const response = deferred<{ captions: string[] }>();
    mockCaptions.mockImplementationOnce(() => response.promise);
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    await addTargets(file('a.png', 'a'), file('b.png', 'b'), file('c.png', 'c'));

    fireEvent.click(screen.getByRole('button', { name: 'Auto-caption images' }));
    await waitFor(() => expect(mockCaptions).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: 'Move target 3: c.png up' }));
    fireEvent.click(screen.getByRole('button', { name: 'Remove target 1: a.png' }));
    fireEvent.change(screen.getByLabelText('Caption for b.png'), {
      target: { value: 'keep my caption' },
    });
    fireEvent.change(screen.getByLabelText('Target images'), {
      target: { files: [file('d.png', 'd')] },
    });
    await screen.findByLabelText('Caption for d.png');

    response.resolve({ captions: ['generated a', 'generated b', 'generated c'] });

    await waitFor(() => {
      expect(screen.getByLabelText('Caption for c.png')).toHaveValue('generated c');
    });
    expect(screen.getByLabelText('Caption for b.png')).toHaveValue('keep my caption');
    expect(screen.getByLabelText('Caption for d.png')).toHaveValue('');
  });

  it('reserves rapid target selections in order while their file reads resolve out of order', async () => {
    const pending = deferFileReads();
    try {
      render(<TrainPanel onTrained={mock()} />);
      await waitFor(() =>
        expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit')
      );
      fillIdentity();
      const first = file('first.png', 'first');
      const second = file('second.png', 'second');

      fireEvent.change(screen.getByLabelText('Target images'), {
        target: { files: [first] },
      });
      fireEvent.change(screen.getByLabelText('Target images'), {
        target: { files: [second] },
      });

      await waitFor(() => {
        expect(screen.getAllByRole('group', { name: /target pair/i })).toHaveLength(2);
      });
      expect(screen.getByLabelText('Instruction for target 1: first.png')).toBeInTheDocument();
      expect(screen.getByLabelText('Instruction for target 2: second.png')).toBeInTheDocument();
      expect(document.activeElement).toBe(
        screen.getByLabelText('Reference image for target 1: first.png')
      );
      expect(screen.getByRole('button', { name: 'Train' })).toBeDisabled();

      const firstRead = pending.reads.find((read) => read.file === first);
      const secondRead = pending.reads.find((read) => read.file === second);
      expect(firstRead).toBeDefined();
      expect(secondRead).toBeDefined();
      secondRead?.complete(Buffer.from('second').toString('base64'));
      firstRead?.complete(Buffer.from('first').toString('base64'));
      pending.restore();

      await selectBase('FLUX.1 Schnell');
      await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
      fireEvent.click(screen.getByRole('button', { name: 'Train' }));
      await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
      expect(
        mockTrain.mock.calls[0]?.[0].images.map((image) => [image.filename, image.bytes_base64])
      ).toEqual([
        ['first.png', Buffer.from('first').toString('base64')],
        ['second.png', Buffer.from('second').toString('base64')],
      ]);
    } finally {
      pending.restore();
    }
  });

  it('keeps the latest reference when rapid file reads resolve out of order', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fillIdentity();
    await addTargets(file('after.png', 'after'));
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after.png'), {
      target: { value: 'turn the shirt blue' },
    });

    const pending = deferFileReads();
    try {
      const first = file('first-before.png', 'first before');
      const second = file('second-before.png', 'second before');
      const reference = screen.getByLabelText('Reference image for target 1: after.png');
      fireEvent.change(reference, { target: { files: [first] } });
      fireEvent.change(reference, { target: { files: [second] } });
      expect(screen.getByText('Reference: second-before.png')).toBeInTheDocument();

      const firstRead = pending.reads.find((read) => read.file === first);
      const secondRead = pending.reads.find((read) => read.file === second);
      expect(firstRead).toBeDefined();
      expect(secondRead).toBeDefined();
      secondRead?.complete(Buffer.from('second before').toString('base64'));
      firstRead?.complete(Buffer.from('first before').toString('base64'));
      pending.restore();

      await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
      fireEvent.click(screen.getByRole('button', { name: 'Train' }));
      await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
      expect(mockTrain.mock.calls[0]?.[0].images[0]?.before_base64).toBe(
        Buffer.from('second before').toString('base64')
      );
    } finally {
      pending.restore();
    }
  });

  it('freezes the submitted draft and exposes busy semantics until training completes', async () => {
    const response = deferred<{
      filename: string;
      quality: null;
      dataset: never[];
    }>();
    mockTrain.mockImplementationOnce(() => response.promise);
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fillIdentity();
    await addTargets(file('after.png', 'after'));
    await addReference('Reference image for target 1: after.png', file('before.png', 'before'));
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after.png'), {
      target: { value: 'turn the shirt blue' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));

    const form = screen.getByRole('button', { name: 'Train' }).closest('form');
    expect(form).toHaveAttribute('aria-busy', 'true');
    for (const control of [
      screen.getByLabelText('Name'),
      screen.getByLabelText('Base'),
      screen.getByLabelText('Trigger word'),
      screen.getByLabelText('Target images'),
      screen.getByLabelText('Reference image for target 1: after.png'),
      screen.getByLabelText('Instruction for target 1: after.png'),
      screen.getByRole('button', { name: 'Remove target 1: after.png' }),
      screen.getByRole('button', { name: 'Train' }),
    ]) {
      expect(control).toBeDisabled();
    }

    response.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] });
    await screen.findByText('Training finished: zoneface.safetensors');
    expect(form).toHaveAttribute('aria-busy', 'false');
    expect(screen.getByLabelText('Name')).toBeEnabled();
    expect(screen.queryByLabelText('Instruction for target 1: after.png')).toBeNull();
  });
});

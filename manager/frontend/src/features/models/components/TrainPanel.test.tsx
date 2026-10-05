import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';

function fluxQwenBases() {
  return [
    { id: 'qwen-image-edit', label: 'Qwen Image Edit', edit: true },
    { id: 'flux-schnell', label: 'FLUX.1 Schnell', edit: false },
  ];
}

function peopleReadyBases() {
  return [
    ...fluxQwenBases(),
    { id: 'sdxl-people', label: 'SDXL people', edit: false, finetune: true },
  ];
}

function languageBases() {
  return [
    ...fluxQwenBases(),
    {
      id: 'qwen2.5:32b',
      label: 'qwen2.5 32B',
      edit: false,
      subject: 'language' as const,
      finetune: false,
    },
    {
      id: 'llama3.2:1b',
      label: 'llama3.2 1B',
      edit: false,
      subject: 'language' as const,
      finetune: true,
    },
  ];
}

const mockTrainBases = mock(() => Promise.resolve(fluxQwenBases()));
const mockCaptions = mock(() => Promise.resolve({ captions: ['generated caption'] }));
const mockFrames = mock(() =>
  Promise.resolve({
    sampled: 32,
    sampled_fps: 8,
    frames: [
      {
        filename: 'frame-0000.png',
        bytes_base64: 'aaa',
        timestamp_ms: 0,
        mirrored: false,
        group: 0,
      },
      {
        filename: 'frame-0001.png',
        bytes_base64: 'bbb',
        timestamp_ms: 250,
        mirrored: true,
        group: 0,
      },
    ],
  })
);
const mockTrain = mock(() =>
  Promise.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] })
);
const mockTrainJob = mock(() => Promise.resolve(null));
const mockWaitTrain = mock(() =>
  Promise.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] })
);

mock.module('../../../api/models', () => ({
  modelsApi: {
    trainBases: mockTrainBases,
    captions: mockCaptions,
    frames: mockFrames,
    train: mockTrain,
    trainJob: mockTrainJob,
    waitTrain: mockWaitTrain,
  },
}));

const mockGetEffectiveAiSettings = mock(() => Promise.resolve({ has_runpod_api_key: false }));

mock.module('../../../api/client', () => ({
  client: {
    getEffectiveAiSettings: mockGetEffectiveAiSettings,
  },
}));

mock.module('../../../shared/context/WorkspaceContext', () => ({
  useWorkspace: () => ({
    organizations: [{ id: 'org-1', name: 'Test Org' }],
    currentOrganization: { id: 'org-1', name: 'Test Org' },
    currentWorkspace: { id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' },
    workspaces: [{ id: 'ws-1', name: 'Test Workspace', organization_id: 'org-1' }],
    loading: false,
    error: null,
    setCurrentOrganization: mock(),
    setCurrentWorkspace: mock(),
    refreshOrganizations: mock(),
    refreshWorkspaces: mock(),
  }),
  WorkspaceProvider: ({ children }: { children: ReactNode }) => children,
}));

let TrainPanel: typeof import('./TrainPanel').default;

beforeAll(async () => {
  TrainPanel = (await import('./TrainPanel')).default;
});

beforeEach(() => {
  mockCaptions.mockClear();
  mockFrames.mockClear();
  mockTrain.mockClear();
  mockTrainJob.mockClear();
  mockWaitTrain.mockClear();
  mockTrainBases.mockImplementation(() => Promise.resolve(fluxQwenBases()));
  mockGetEffectiveAiSettings.mockClear();
  mockGetEffectiveAiSettings.mockImplementation(() =>
    Promise.resolve({ has_runpod_api_key: false })
  );
  mockTrain.mockImplementation(() =>
    Promise.resolve({ filename: 'zoneface.safetensors', quality: null, dataset: [] })
  );
  mockTrainJob.mockImplementation(() => Promise.resolve(null));
  mockWaitTrain.mockImplementation(() =>
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

async function selectOption(label: string, name: string): Promise<void> {
  fireEvent.click(screen.getByLabelText(label));
  fireEvent.click(await screen.findByRole('option', { name }));
  await waitFor(() => {
    expect(screen.getByLabelText(label)).toHaveTextContent(name);
  });
}

async function selectBase(label: string): Promise<void> {
  await selectOption('Base', label);
}

async function selectSubject(label: string): Promise<void> {
  await selectOption('Subject', label);
}

async function selectMethod(label: string): Promise<void> {
  await selectOption('Method', label);
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

async function blobText(blob: Blob | undefined): Promise<string> {
  expect(blob).toBeDefined();
  return blob?.text() ?? '';
}

describe('TrainPanel', () => {
  it('receipts an accepted clip under the Video zone and clears its frames from there', async () => {
    const pending = deferred<Awaited<ReturnType<typeof mockFrames>>>();
    mockFrames.mockImplementationOnce(() => pending.promise);
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    });
    await selectBase('FLUX.1 Schnell');
    const field = screen.getByLabelText('Video').closest('.drop-zone-field') as HTMLElement;

    fireEvent.change(screen.getByLabelText('Video'), {
      target: { files: [new File(['clip'], 'subject.mp4', { type: 'video/mp4' })] },
    });
    expect(field.contains(await screen.findByText('Reading subject.mp4…'))).toBe(true);
    expect(screen.getByLabelText('Video')).toBeEnabled();

    pending.resolve({
      sampled: 32,
      sampled_fps: 8,
      frames: [
        {
          filename: 'frame-0000.png',
          bytes_base64: 'aaa',
          timestamp_ms: 0,
          mirrored: false,
          group: 0,
        },
        {
          filename: 'frame-0001.png',
          bytes_base64: 'bbb',
          timestamp_ms: 250,
          mirrored: true,
          group: 0,
        },
      ],
    });
    const receipt = await screen.findByText(/32 frames read at 8\.0\/s, 2 kept/);
    expect(receipt).toHaveTextContent('subject.mp4: 32 frames read at 8.0/s, 2 kept');
    expect(field.contains(receipt)).toBe(true);
    expect(screen.queryByText('Reading subject.mp4…')).toBeNull();
    expect(screen.getAllByRole('group', { name: /target pair/i })).toHaveLength(2);
    const mirror = screen.getByLabelText('Mirror half the frames of each second');
    expect(mirror.compareDocumentPosition(receipt) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();

    const remove = screen.getByRole('button', { name: 'Remove clip subject.mp4' });
    expect(field.contains(remove)).toBe(true);
    fireEvent.click(remove);
    await waitFor(() => {
      expect(screen.queryAllByRole('group', { name: /target pair/i })).toHaveLength(0);
    });
    expect(screen.queryByText(/frames read at/)).toBeNull();
    expect(screen.getByLabelText('Video').closest('.drop-zone')).toHaveTextContent(
      'Drop clips or a folder'
    );
  });

  it('titles each target inside its own row instead of on the fieldset border', async () => {
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    });
    await selectBase('FLUX.1 Schnell');
    await addTargets(file('frame-0000.png', 'a'), file('frame-0001.png', 'b'));

    expect(document.querySelector('legend')).toBeNull();
    const [first] = screen.getAllByRole('group', { name: 'Target pair 1: frame-0000.png' });
    const head = first.querySelector('.train-pair-head');
    expect(head).toHaveTextContent('Target 1');
    expect(head).toHaveTextContent('frame-0000.png');
    expect(first.querySelector('.train-pair-thumb img')?.getAttribute('src')).toMatch(/^blob:/);
    expect(screen.getByLabelText('Caption for frame-0000.png')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Move target 1: frame-0000.png up' })).toBeDisabled();
    expect(
      screen.getByRole('button', { name: 'Move target 2: frame-0001.png down' })
    ).toBeDisabled();
  });

  it('lays the form out as paired drop zones, an identity row and a right-aligned footer', async () => {
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    });
    await selectBase('FLUX.1 Schnell');

    const targets = screen.getByLabelText('Target images');
    const clips = screen.getByLabelText('Video');
    expect(targets.closest('.drop-zone')).toHaveTextContent('Drop images, clips, or a folder');
    expect(clips.closest('.drop-zone')).toHaveTextContent('Drop clips or a folder');
    expect(targets.closest('.train-drops')).toBe(clips.closest('.train-drops'));
    expect(targets.closest('.train-drops')).not.toHaveClass('train-drops--single');
    expect(screen.getByText('Target images')).toHaveAttribute(
      'id',
      targets.getAttribute('aria-labelledby')
    );
    expect(screen.getByText('Video', { selector: 'label' })).toHaveAttribute(
      'id',
      clips.getAttribute('aria-labelledby')
    );
    expect(
      screen.getByText(
        'Choose the images this LoRA should learn from. Clips and folders are fine too.'
      )
    ).toHaveAttribute('id', targets.getAttribute('aria-describedby'));
    expect(document.querySelector('.ui-input[type="file"]')).toBeNull();

    const identity = screen.getByLabelText('Name').closest('.train-identity');
    expect(identity).not.toBeNull();
    expect(screen.getByLabelText('Trigger word').closest('.train-identity')).toBe(identity);
    expect(screen.getByLabelText('Base').closest('.train-identity')).toBeNull();
    const kind = screen.getByLabelText('Subject').closest('.train-kind');
    expect(kind).not.toBeNull();
    expect(screen.getByLabelText('Method').closest('.train-kind')).toBe(kind);
    expect(screen.getByLabelText('Compute').closest('.train-kind')).toBe(kind);
    expect(screen.getByLabelText('Compute')).toHaveTextContent('Local');
    expect(screen.getByLabelText('Name').closest('.train-kind')).toBeNull();
    expect(screen.getByLabelText('Subject').closest('.train-identity')).toBeNull();

    const train = screen.getByRole('button', { name: 'Train' });
    expect(train.parentElement).toHaveClass('train-footer');

    await addTargets(file('frame-0000.png', 'a'));
    const caption = screen.getByRole('button', { name: 'Auto-caption images' });
    expect(caption.parentElement).toHaveClass('train-caption');
    expect(caption.nextElementSibling).toHaveClass('train-caption-hint');

    await selectBase('Qwen Image Edit');
    expect(screen.queryByLabelText('Video')).toBeNull();
    expect(screen.getByLabelText('Target images').closest('.train-drops')).toHaveClass(
      'train-drops--single'
    );
  });

  it('adds dropped images as targets and ignores files the zone does not accept', async () => {
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    });
    await selectBase('FLUX.1 Schnell');

    const zone = screen.getByLabelText('Target images').closest('.drop-zone') as HTMLElement;
    fireEvent.dragOver(zone);
    expect(zone).toHaveClass('drop-zone--over');
    fireEvent.drop(zone, {
      dataTransfer: {
        files: [file('frame-0000.png', 'a'), new File(['x'], 'notes.txt', { type: 'text/plain' })],
      },
    });
    await waitFor(() => {
      expect(screen.getAllByRole('group', { name: /target pair/i })).toHaveLength(1);
    });
    expect(
      screen.getByRole('group', { name: 'Target pair 1: frame-0000.png' })
    ).toBeInTheDocument();
    expect(zone).not.toHaveClass('drop-zone--over');
  });

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
    expect(screen.getByLabelText('Subject').getAttribute('role')).toBe('combobox');
    expect(screen.getByLabelText('Method').getAttribute('role')).toBe('combobox');
    expect(screen.getByLabelText('Compute').getAttribute('role')).toBe('combobox');
    expect(screen.getByLabelText('Subject')).toHaveTextContent('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    expect(screen.getByLabelText('Compute')).toHaveTextContent('Local');
    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();
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
    expect(request?.images[0]?.before).toBeTruthy();
    expect(request?.images[1]).toMatchObject({
      filename: 'after-two.png',
      caption: 'move the subject outside',
    });
    expect(request?.images[1]?.before).toBeTruthy();
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
            {
              source_index: 0,
              filename: 'small.png',
              reason: 'small' as const,
              outcome: 'used' as const,
            },
            {
              source_index: 1,
              filename: 'blurred.png',
              reason: 'blurred' as const,
              outcome: 'still_rejected' as const,
            },
            {
              source_index: 2,
              filename: 'broken.png',
              reason: 'small' as const,
              outcome: 'failed' as const,
            },
          ],
        },
      })
    );
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    fillIdentity();
    await addTargets(file('target.png', 'target'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));

    await screen.findByText('Image repairs');
    expect(
      screen.getByText('Zone tried to improve these images before the final screen:')
    ).toBeInTheDocument();
    expect(
      screen.getByText(/It passed the final screen, and the improved copy was used\./)
    ).toBeInTheDocument();
    expect(
      screen.getByText(/It did not pass the final screen, so it was not used\./)
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Zone could not create an improved copy, so it was not used\./)
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

  it('keeps captions from finished batches when a later batch fails', async () => {
    mockCaptions
      .mockImplementationOnce(async (body: { images: Array<{ filename: string }> }) => ({
        captions: body.images.map((image) => `caption ${image.filename}`),
      }))
      .mockImplementationOnce(() => Promise.reject(new Error('captioning failed')));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    const files = Array.from({ length: 25 }, (_, index) =>
      file(`${String(index).padStart(2, '0')}.png`, `img-${index}`)
    );
    fireEvent.change(screen.getByLabelText('Target images'), { target: { files } });
    await waitFor(() => {
      expect(screen.getByLabelText('Caption for 00.png')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Auto-caption images' }));

    await waitFor(() => expect(mockCaptions).toHaveBeenCalledTimes(2));
    await waitFor(() => {
      expect(screen.getByText('captioning failed')).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Caption for 00.png')).toHaveValue('caption 00.png');
  });

  it('keeps rapid target selections in the order they were added', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
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

    await selectBase('FLUX.1 Schnell');
    await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    const uploaded = mockTrain.mock.calls[0]?.[0].images ?? [];
    expect(uploaded.map((image) => image.filename)).toEqual(['first.png', 'second.png']);
    expect(await blobText(uploaded[0]?.blob)).toBe('first');
    expect(await blobText(uploaded[1]?.blob)).toBe('second');
  });

  it('keeps the latest reference when another is chosen immediately', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    fillIdentity();
    await addTargets(file('after.png', 'after'));
    fireEvent.change(screen.getByLabelText('Instruction for target 1: after.png'), {
      target: { value: 'turn the shirt blue' },
    });

    const first = file('first-before.png', 'first before');
    const second = file('second-before.png', 'second before');
    const reference = screen.getByLabelText('Reference image for target 1: after.png');
    fireEvent.change(reference, { target: { files: [first] } });
    fireEvent.change(reference, { target: { files: [second] } });
    expect(screen.getByText('Reference: second-before.png')).toBeInTheDocument();

    await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(await blobText(mockTrain.mock.calls[0]?.[0].images[0]?.before)).toBe('second before');
  });

  it('accepts a dump of empty-type images without reading them as data URLs', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    fillIdentity();
    const dumped = Array.from(
      { length: 80 },
      (_, index) => new File([`shot-${index}`], `shot-${index}.png`)
    );
    fireEvent.change(screen.getByLabelText('Target images'), {
      target: { files: dumped },
    });
    expect(
      await screen.findByRole('group', { name: 'Target pair 1: shot-0.png' })
    ).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: /target pair 80/i })).toBeNull();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0].images).toHaveLength(80);
  });

  it('partitions a mixed drop of photos and clips', async () => {
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    const zone = screen.getByLabelText('Target images').closest('.drop-zone') as HTMLElement;
    fireEvent.drop(zone, {
      dataTransfer: {
        files: [
          file('portrait.png', 'a'),
          new File(['clip'], 'walk.mp4', { type: 'video/mp4' }),
          new File(['x'], 'notes.txt', { type: 'text/plain' }),
        ],
      },
    });
    expect(
      await screen.findByRole('group', { name: 'Target pair 1: portrait.png' })
    ).toBeInTheDocument();
    const clips = await screen.findByLabelText('Accepted clips');
    expect(clips).toHaveTextContent('walk.mp4: 32 frames read at 8.0/s, 2 kept');
    expect(screen.getAllByRole('group', { name: /target pair/i })).toHaveLength(3);
    expect(mockFrames).toHaveBeenCalledTimes(1);
    expect(mockFrames.mock.calls[0]?.[0].filename).toBe('walk.mp4');
    expect(mockFrames.mock.calls[0]?.[0].blob).toBeInstanceOf(Blob);
  });

  it('extracts several clips at once and leaves the video zone enabled', async () => {
    const first = deferred<Awaited<ReturnType<typeof mockFrames>>>();
    const second = deferred<Awaited<ReturnType<typeof mockFrames>>>();
    mockFrames
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);
    render(<TrainPanel onTrained={() => {}} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    fireEvent.change(screen.getByLabelText('Video'), {
      target: {
        files: [
          new File(['one'], 'one.mp4', { type: 'video/mp4' }),
          new File(['two'], 'two.mp4', { type: 'video/mp4' }),
        ],
      },
    });
    expect(await screen.findByText('Reading 2 clips…')).toBeInTheDocument();
    expect(screen.getByLabelText('Video')).toBeEnabled();
    await waitFor(() => expect(mockFrames).toHaveBeenCalledTimes(2));

    const clip = {
      sampled: 8,
      sampled_fps: 4,
      frames: [
        {
          filename: 'frame-0000.png',
          bytes_base64: 'YWFh',
          timestamp_ms: 0,
          mirrored: false,
          group: 0,
        },
      ],
    };
    first.resolve(clip);
    second.resolve(clip);
    const clips = await screen.findByLabelText('Accepted clips');
    expect(clips).toHaveTextContent('one.mp4: 8 frames read at 4.0/s, 1 kept');
    expect(clips).toHaveTextContent('two.mp4: 8 frames read at 4.0/s, 1 kept');
    expect(screen.queryByText(/Reading/)).toBeNull();
    expect(screen.getAllByRole('group', { name: /target pair/i })).toHaveLength(2);
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
      screen.getByLabelText('Subject'),
      screen.getByLabelText('Method'),
      screen.getByLabelText('Compute'),
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

  it('resumes a running job when the page is opened', async () => {
    const trained = mock();
    mockTrainJob.mockImplementationOnce(() =>
      Promise.resolve({
        id: 'job-1',
        name: 'jerry',
        status: 'running',
        filename: null,
        quality: null,
        step: 40,
        total: 400,
        eta_seconds: 180,
      })
    );
    const response = deferred<{ filename: string; quality: null; dataset: never[] }>();
    mockWaitTrain.mockImplementationOnce(() => response.promise);
    render(<TrainPanel onTrained={trained} />);

    await waitFor(() => expect(screen.getByText('Training jerry')).toBeInTheDocument());
    expect(screen.getByText('Step 40 of 400')).toBeInTheDocument();
    expect(screen.getByText(/about 3 minutes left/)).toBeInTheDocument();
    expect(screen.getByText(/This run keeps going if you leave the page/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Train' })).toBeDisabled();

    response.resolve({ filename: 'jerry.safetensors', quality: null, dataset: [] });
    await screen.findByText('Training finished: jerry.safetensors');
    expect(trained).toHaveBeenCalled();
  });

  it('trains a person LoRA on the SDXL people base and hides Flux and Qwen', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');

    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('SDXL people');
    });
    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();
    expect(screen.getByText(/~100–200 MB adapter/)).toBeInTheDocument();
    expect(screen.getByText(/1024 buckets, rank 64/)).toBeInTheDocument();
    expect(screen.getByText(/20–200 unique stills/)).toBeInTheDocument();
    expect(screen.getByLabelText('Video')).toBeInTheDocument();
    expect(screen.queryByLabelText(/Reference image/)).toBeNull();

    fireEvent.click(screen.getByLabelText('Base'));
    expect(screen.queryByRole('option', { name: 'FLUX.1 Schnell' })).toBeNull();
    expect(screen.queryByRole('option', { name: 'Qwen Image Edit' })).toBeNull();
    fireEvent.click(screen.getByRole('option', { name: 'SDXL people' }));

    fireEvent.click(screen.getByLabelText('Method'));
    expect(await screen.findByRole('option', { name: 'Fine-tune' })).not.toHaveAttribute(
      'data-disabled'
    );
    fireEvent.click(screen.getByRole('option', { name: 'LoRA' }));

    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      name: 'zoneface',
      base: 'sdxl-people',
      trigger: 'zne person',
      subject: 'person',
      method: 'lora',
      provider: 'local',
      workspace_id: 'ws-1',
    });
  });

  it('submits a person fine-tune and retitles the panel', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    await selectMethod('Fine-tune');

    expect(screen.getByRole('heading', { name: 'Fine-tune a person' })).toBeInTheDocument();
    expect(screen.getByText(/~7 GB checkpoint/)).toBeInTheDocument();
    expect(screen.getByText(/full UNet \+ CLIP-L/)).toBeInTheDocument();
    expect(screen.getByText(/resumes after a refresh or restart/)).toBeInTheDocument();
    expect(screen.getAllByText(/200 unique stills or 20 clips/).length).toBeGreaterThan(0);
    expect(screen.getByText(/train a LoRA first/)).toBeInTheDocument();

    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      base: 'sdxl-people',
      subject: 'person',
      method: 'finetune',
    });
  });

  it('keeps Fine-tune disabled while Subject is Other', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    expect(screen.getByLabelText('Subject')).toHaveTextContent('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');

    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Method'));
    const fineTune = await screen.findByRole('option', { name: 'Fine-tune' });
    expect(fineTune).toHaveAttribute('data-disabled');
    fireEvent.click(fineTune);
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    fireEvent.click(screen.getByRole('option', { name: 'LoRA' }));
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
  });

  it('submits a person pivotal train and retitles the panel', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    fireEvent.click(screen.getByLabelText('Method'));
    expect(await screen.findByRole('option', { name: 'Pivotal' })).not.toHaveAttribute(
      'data-disabled'
    );
    fireEvent.click(screen.getByRole('option', { name: 'Pivotal' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Method')).toHaveTextContent('Pivotal');
    });

    expect(screen.getByRole('heading', { name: 'Pivotal training' })).toBeInTheDocument();
    expect(screen.getByText(/textual inversion/)).toBeInTheDocument();
    expect(screen.getByText(/CLIP-L/)).toBeInTheDocument();

    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      base: 'sdxl-people',
      subject: 'person',
      method: 'pivotal',
    });
  });

  it('submits a person video train and retitles the panel', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    fireEvent.click(screen.getByLabelText('Method'));
    expect(await screen.findByRole('option', { name: 'Video' })).not.toHaveAttribute(
      'data-disabled'
    );
    fireEvent.click(screen.getByRole('option', { name: 'Video' }));
    await waitFor(() => {
      expect(screen.getByLabelText('Method')).toHaveTextContent('Video');
    });

    expect(screen.getByRole('heading', { name: 'Train video identity' })).toBeInTheDocument();
    expect(screen.getByText(/Wan 2\.2 TI2V 5B/)).toBeInTheDocument();
    expect(screen.getAllByText(/2–3 windows/).length).toBeGreaterThan(0);

    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      base: 'sdxl-people',
      subject: 'person',
      method: 'video',
    });
  });

  it('keeps Pivotal and Video disabled while Subject is Other', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    expect(screen.getByLabelText('Subject')).toHaveTextContent('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');

    fireEvent.click(screen.getByLabelText('Method'));
    const pivotal = await screen.findByRole('option', { name: 'Pivotal' });
    const video = screen.getByRole('option', { name: 'Video' });
    expect(pivotal).toHaveAttribute('data-disabled');
    expect(video).toHaveAttribute('data-disabled');
    fireEvent.click(pivotal);
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    fireEvent.click(screen.getByRole('option', { name: 'Video' }));
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    fireEvent.click(screen.getByRole('option', { name: 'LoRA' }));
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
  });

  it('disables Train when Person is chosen without the SDXL people bundle', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');

    expect(
      screen.getByText(/The SDXL people bundle must be downloaded before training a person/)
    ).toBeInTheDocument();
    expect(
      screen.getByText(/setup-comfyui-macos.sh --download-model --bundle image-people/)
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Base')).toBeDisabled();
    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    expect(screen.getByRole('button', { name: 'Train' })).toBeDisabled();
    expect(mockTrain).not.toHaveBeenCalled();
  });

  it('restores a Flux or Qwen base and LoRA when switching Person back to Other', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectBase('FLUX.1 Schnell');
    await selectSubject('Person');
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('SDXL people');
    });
    await selectMethod('Fine-tune');
    expect(screen.getByRole('heading', { name: 'Fine-tune a person' })).toBeInTheDocument();

    await selectSubject('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();
    expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit');
    fireEvent.click(screen.getByLabelText('Base'));
    expect(screen.getByRole('option', { name: 'FLUX.1 Schnell' })).toBeInTheDocument();
    expect(screen.queryByRole('option', { name: 'SDXL people' })).toBeNull();
    fireEvent.click(screen.getByRole('option', { name: 'Qwen Image Edit' }));
  });

  it('restores LoRA when switching Person back to Other after Pivotal or Video', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    await selectMethod('Pivotal');
    expect(screen.getByRole('heading', { name: 'Pivotal training' })).toBeInTheDocument();

    await selectSubject('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();

    await selectSubject('Person');
    await selectMethod('Video');
    expect(screen.getByRole('heading', { name: 'Train video identity' })).toBeInTheDocument();

    await selectSubject('Other');
    expect(screen.getByLabelText('Method')).toHaveTextContent('LoRA');
    expect(screen.getByRole('heading', { name: 'Train a LoRA' })).toBeInTheDocument();
  });

  it('trains a language LoRA from documents without a trigger', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(languageBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await addTargets(file('portrait.png', 'portrait'));
    expect(screen.getByRole('group', { name: /target pair/i })).toBeInTheDocument();

    await selectSubject('Language');
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('qwen2.5 32B');
    });
    expect(screen.getByRole('heading', { name: 'Train a language LoRA' })).toBeInTheDocument();
    expect(screen.getByLabelText('Method')).toHaveTextContent('Language LoRA');
    expect(screen.getByText(/installed chat model/)).toBeInTheDocument();
    expect(screen.getByText(/under 8B/)).toBeInTheDocument();
    expect(screen.getByText(/Large bases stay language LoRA/)).toBeInTheDocument();
    expect(screen.queryByLabelText('Trigger word')).toBeNull();
    expect(screen.queryByLabelText('Video')).toBeNull();
    expect(screen.queryByLabelText('Mirror half the frames of each second')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Auto-caption images' })).toBeNull();
    expect(screen.queryByRole('group', { name: /target pair/i })).toBeNull();
    fireEvent.click(screen.getByLabelText('Base'));
    expect(screen.queryByRole('option', { name: 'FLUX.1 Schnell' })).toBeNull();
    expect(screen.queryByRole('option', { name: 'Qwen Image Edit' })).toBeNull();
    expect(screen.queryByRole('option', { name: 'SDXL people' })).toBeNull();
    fireEvent.click(screen.getByRole('option', { name: 'qwen2.5 32B' }));

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'notes' } });
    const dump = new File(['{"text":"hello"}\n'], 'notes.jsonl', { type: 'application/json' });
    fireEvent.change(screen.getByLabelText('Documents'), { target: { files: [dump] } });
    await waitFor(() => expect(screen.getByText('notes.jsonl')).toBeInTheDocument());
    expect(screen.getByLabelText('Training documents')).toBeInTheDocument();
    expect(screen.queryByRole('group', { name: /target pair/i })).toBeNull();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());

    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    const request = mockTrain.mock.calls[0]?.[0];
    expect(request).toMatchObject({
      name: 'notes',
      base: 'qwen2.5:32b',
      subject: 'language',
      method: 'lora',
      provider: 'local',
    });
    expect(request?.trigger).toBeUndefined();
    expect(request?.images).toHaveLength(1);
    expect(request?.images[0]).toMatchObject({ filename: 'notes.jsonl', caption: '' });
    expect(await blobText(request?.images[0]?.blob)).toBe('{"text":"hello"}\n');
  });

  it('disables Fine-tune on a large chat base and enables it on a 1B', async () => {
    mockTrainBases.mockImplementation(() => Promise.resolve(languageBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Language');
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('qwen2.5 32B');
    });

    fireEvent.click(screen.getByLabelText('Method'));
    const disabledFineTune = await screen.findByRole('option', { name: 'Fine-tune' });
    expect(disabledFineTune).toHaveAttribute('data-disabled');
    expect(screen.getByRole('option', { name: 'Pivotal' })).toHaveAttribute('data-disabled');
    expect(screen.getByRole('option', { name: 'Video' })).toHaveAttribute('data-disabled');
    fireEvent.click(disabledFineTune);
    expect(screen.getByLabelText('Method')).toHaveTextContent('Language LoRA');
    fireEvent.click(screen.getByRole('option', { name: 'Language LoRA' }));

    await selectBase('llama3.2 1B');
    fireEvent.click(screen.getByLabelText('Method'));
    const enabledFineTune = await screen.findByRole('option', { name: 'Fine-tune' });
    expect(enabledFineTune).not.toHaveAttribute('data-disabled');
    fireEvent.click(enabledFineTune);
    await waitFor(() => {
      expect(screen.getByLabelText('Method')).toHaveTextContent('Fine-tune');
    });
    expect(screen.getByRole('heading', { name: 'Fine-tune a chat model' })).toBeInTheDocument();
  });

  it('asks to install a chat model when none are listed', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Language');
    expect(screen.getByText('Install a chat model on Ollama before training.')).toBeInTheDocument();
    expect(screen.getByLabelText('Base')).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'notes' } });
    fireEvent.change(screen.getByLabelText('Documents'), {
      target: { files: [new File(['hello'], 'notes.txt', { type: 'text/plain' })] },
    });
    await waitFor(() => expect(screen.getByText('notes.txt')).toBeInTheDocument());
    expect(screen.getByRole('button', { name: 'Train' })).toBeDisabled();
    expect(mockTrain).not.toHaveBeenCalled();
  });

  it('disables Runpod until a Runpod key is saved', async () => {
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    expect(screen.getByLabelText('Subject')).toHaveTextContent('Other');
    expect(screen.getByText('Save a Runpod API key in Workspace Settings.')).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText('Compute'));
    expect(await screen.findByRole('option', { name: 'Runpod' })).toHaveAttribute('data-disabled');
    fireEvent.click(screen.getByRole('option', { name: 'Local' }));
    expect(screen.getByLabelText('Compute')).toHaveTextContent('Local');
  });

  it('posts Runpod compute for a Person fine-tune when a key is saved', async () => {
    mockGetEffectiveAiSettings.mockImplementation(() =>
      Promise.resolve({ has_runpod_api_key: true })
    );
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    await waitFor(() => {
      expect(screen.queryByText('Save a Runpod API key in Workspace Settings.')).toBeNull();
    });
    await selectOption('Compute', 'Runpod');
    await selectMethod('Fine-tune');
    expect(screen.getByText(/Auto-picks a 48 GB GPU \(A40 class\)/)).toBeInTheDocument();
    expect(screen.getByText(/About 4 hours, \$1–2/)).toBeInTheDocument();

    fillIdentity();
    await addTargets(file('portrait.png', 'portrait'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      subject: 'person',
      method: 'finetune',
      provider: 'runpod',
      workspace_id: 'ws-1',
    });
  });

  it('quotes a 24 GB GPU for Runpod LoRA, pivotal, and video', async () => {
    mockGetEffectiveAiSettings.mockImplementation(() =>
      Promise.resolve({ has_runpod_api_key: true })
    );
    mockTrainBases.mockImplementation(() => Promise.resolve(peopleReadyBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Person');
    await waitFor(() => {
      expect(screen.queryByText('Save a Runpod API key in Workspace Settings.')).toBeNull();
    });
    await selectOption('Compute', 'Runpod');
    expect(screen.getByText('A 24 GB GPU is enough for this method.')).toBeInTheDocument();
    await selectMethod('Pivotal');
    expect(screen.getByText('A 24 GB GPU is enough for this method.')).toBeInTheDocument();
    await selectMethod('Video');
    expect(screen.getByText('A 24 GB GPU is enough for this method.')).toBeInTheDocument();
  });

  it('posts Runpod compute for a language LoRA when a key is saved', async () => {
    mockGetEffectiveAiSettings.mockImplementation(() =>
      Promise.resolve({ has_runpod_api_key: true })
    );
    mockTrainBases.mockImplementation(() => Promise.resolve(languageBases()));
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await selectSubject('Language');
    await waitFor(() => {
      expect(screen.getByLabelText('Base')).toHaveTextContent('qwen2.5 32B');
    });
    await waitFor(() => {
      expect(screen.queryByText('Save a Runpod API key in Workspace Settings.')).toBeNull();
    });
    await selectOption('Compute', 'Runpod');
    expect(screen.getByText('A 24 GB GPU is enough for this method.')).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'notes' } });
    const dump = new File(['{"text":"hello"}\n'], 'notes.jsonl', { type: 'application/json' });
    fireEvent.change(screen.getByLabelText('Documents'), { target: { files: [dump] } });
    await waitFor(() => expect(screen.getByText('notes.jsonl')).toBeInTheDocument());
    await waitFor(() => expect(screen.getByRole('button', { name: 'Train' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      subject: 'language',
      method: 'lora',
      provider: 'runpod',
    });
  });

  it('posts Runpod compute for an Other LoRA when a key is saved', async () => {
    mockGetEffectiveAiSettings.mockImplementation(() =>
      Promise.resolve({ has_runpod_api_key: true })
    );
    render(<TrainPanel onTrained={mock()} />);
    await waitFor(() => expect(screen.getByLabelText('Base')).toHaveTextContent('Qwen Image Edit'));
    await waitFor(() => {
      expect(screen.queryByText('Save a Runpod API key in Workspace Settings.')).toBeNull();
    });
    await selectBase('FLUX.1 Schnell');
    await selectOption('Compute', 'Runpod');
    expect(screen.getByText('A 24 GB GPU is enough for this method.')).toBeInTheDocument();
    fillIdentity();
    await addTargets(file('target.png', 'target'));
    fireEvent.click(screen.getByRole('button', { name: 'Train' }));
    await waitFor(() => expect(mockTrain).toHaveBeenCalledTimes(1));
    expect(mockTrain.mock.calls[0]?.[0]).toMatchObject({
      subject: 'other',
      method: 'lora',
      provider: 'runpod',
      workspace_id: 'ws-1',
    });
  });
});

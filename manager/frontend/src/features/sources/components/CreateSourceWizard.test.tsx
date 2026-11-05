import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import type { Source } from '../types';

const mockGetSourceTypes = mock();

mock.module('../../../api/sources', () => ({
  sourcesApi: {
    getSourceTypes: mockGetSourceTypes,
  },
}));

let CreateSourceWizard: typeof import('./CreateSourceWizard').CreateSourceWizard;

beforeAll(async () => {
  ({ CreateSourceWizard } = await import('./CreateSourceWizard'));
});

afterAll(() => {
  mock.restore();
});

const kind = (id: string) => ({ id, name: id, category: 'file', enabled: true });

const offeredKinds = () =>
  Array.from(document.querySelectorAll('.source-type-option .source-type-name')).map(
    (node) => node.textContent
  );

const offeredDescriptions = () =>
  Array.from(document.querySelectorAll('.source-type-option .source-type-desc')).map(
    (node) => node.textContent
  );

function renderWizard() {
  return render(
    <CreateSourceWizard
      isOpen
      onClose={() => {}}
      onCreated={() => {}}
      createSource={() => Promise.reject(new Error('not under test'))}
    />
  );
}

describe('CreateSourceWizard kinds', () => {
  beforeEach(() => {
    mockGetSourceTypes.mockReset();
  });

  it('offers only the kinds the server lists', async () => {
    mockGetSourceTypes.mockResolvedValue([kind('github'), kind('text')]);

    renderWizard();

    await waitFor(() => {
      expect(offeredKinds()).toEqual(['GitHub', 'Text']);
    });
    expect(mockGetSourceTypes).toHaveBeenCalledTimes(1);
  });

  it('describes each kind in a phrase short enough for one line of a tile', async () => {
    mockGetSourceTypes.mockResolvedValue(
      ['github', 'gitlab', 'filesystem', 'web', 'text'].map(kind)
    );

    renderWizard();

    await waitFor(() => {
      expect(offeredDescriptions()).toEqual([
        'GitHub repository',
        'GitLab project',
        'Local directory',
        'Web page',
        'Raw text',
      ]);
    });
  });

  it('never offers a kind without an adapter, even when the server cannot be asked', async () => {
    mockGetSourceTypes.mockRejectedValue(new Error('offline'));

    renderWizard();

    await waitFor(() => {
      expect(mockGetSourceTypes).toHaveBeenCalledTimes(1);
    });
    const kinds = offeredKinds();
    expect(kinds).toEqual(['GitHub', 'GitLab', 'Filesystem', 'Web URL', 'Text']);
    expect(kinds).not.toContain('Email');
    expect(kinds).not.toContain('Calendar');
    expect(screen.queryByText('Notion')).toBeNull();
  });
});

describe('CreateSourceWizard embedding', () => {
  const created = {
    id: 'source-zone',
    name: 'abnegate/zone',
    source_type: 'github',
    category: 'file',
    config: { owner: 'abnegate', repo: 'zone', branch: 'main' },
    url: 'https://github.com/abnegate/zone',
    description: null,
    is_active: true,
    last_verified_at: null,
    last_error: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  } as Source;

  beforeEach(() => {
    mockGetSourceTypes.mockReset();
    mockGetSourceTypes.mockResolvedValue([kind('github')]);
  });

  function Opener({ onCreated }: { onCreated: (source: Source) => void }) {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>
          Open
        </button>
        <CreateSourceWizard
          isOpen={open}
          onClose={() => setOpen(false)}
          onCreated={onCreated}
          createSource={() => Promise.resolve(created)}
        />
      </>
    );
  }

  it('hands the created source to onCreated', async () => {
    const user = userEvent.setup();
    const onCreated = mock((_source: Source) => {});
    render(<Opener onCreated={onCreated} />);
    await user.click(screen.getByRole('button', { name: 'Open' }));
    const dialog = await screen.findByRole('dialog', { name: 'Add Source' });

    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    await user.type(await within(dialog).findByLabelText(/Owner/), 'abnegate');
    await user.type(within(dialog).getByLabelText(/Repository/), 'zone');
    await user.click(within(dialog).getByRole('button', { name: 'Next' }));
    await within(dialog).findByLabelText(/Display Name/);
    await user.click(within(dialog).getByRole('button', { name: 'Add Source' }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(created));
  });

  it('takes focus when it opens and returns it to the opener when it closes', async () => {
    const user = userEvent.setup();
    render(<Opener onCreated={() => {}} />);
    const opener = screen.getByRole('button', { name: 'Open' });
    await user.click(opener);
    const dialog = await screen.findByRole('dialog', { name: 'Add Source' });

    expect(dialog.contains(document.activeElement)).toBe(true);

    await user.keyboard('{Escape}');

    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Add Source' })).toBeNull());
    expect(document.activeElement).toBe(opener);
  });
});

describe('CreateSourceWizard configuration step', () => {
  beforeEach(() => {
    mockGetSourceTypes.mockReset();
    mockGetSourceTypes.mockResolvedValue([kind('github')]);
  });

  it('seats the access token beside the branch and states its hint once', async () => {
    renderWizard();
    await waitFor(() => {
      expect(offeredKinds()).toEqual(['GitHub']);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));

    const branch = await screen.findByLabelText(/Branch/);
    const token = screen.getByLabelText(/Access Token/);
    const row = branch.closest('.form-row');
    expect(row).not.toBeNull();
    expect(token.closest('.form-row')).toBe(row);
    expect(row?.querySelectorAll('.form-group')).toHaveLength(2);
    expect(screen.getAllByText('Token required for private repos and write access')).toHaveLength(
      1
    );

    fireEvent.change(token, { target: { value: 'ghp_secret' } });
    expect(token).toHaveValue('ghp_secret');
    expect(branch).toHaveValue('main');
  });
});

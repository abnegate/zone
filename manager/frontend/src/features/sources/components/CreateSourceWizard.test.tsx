import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

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

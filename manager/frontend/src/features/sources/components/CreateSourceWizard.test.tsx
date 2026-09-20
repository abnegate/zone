import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { render, screen, waitFor } from '@testing-library/react';

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

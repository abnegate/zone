import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../api/ApiError';
import { HttpStatus } from '../../../api/HttpStatus';
import {
  options,
  PREFIXED_SOURCE_ID,
  prefixedSource,
  preview,
  SOURCE_ID,
} from '../../../test/releases';
import { chooseSelect, selectOptionValues } from '../../../test/select';
import { ReleaseRefusal } from '../ReleaseRefusal';
import type { Created, Reconciliation, SourceOptions } from '../types';
import { RECONCILIATION_NEEDED, reconciliationFacts } from '../utils/reconciliation';
import { TagProblem } from '../utils/tags';
import { OPEN_RELEASES } from './ReleaseWizardFailure';

const mockPreview = mock();
const mockCreate = mock();
const mockReset = mock();
const mockReload = mock();

let sources: readonly SourceOptions[] = options.sources;
let optionsLoading = false;
let optionsError: string | null = null;
const opened: boolean[] = [];

mock.module('../hooks/useReleaseOptions', () => ({
  useReleaseOptions: (open: boolean) => {
    opened.push(open);
    return {
      options: undefined,
      sources,
      loading: optionsLoading,
      error: optionsError,
      reload: mockReload,
      reloading: false,
    };
  },
}));

mock.module('../hooks/useCreateRelease', () => ({
  useCreateRelease: () => ({
    preview: mockPreview,
    previewing: false,
    create: mockCreate,
    creating: false,
    reset: mockReset,
  }),
}));

let wizard: typeof import('./CreateReleaseWizard');

beforeAll(async () => {
  wizard = await import('./CreateReleaseWizard');
});

afterAll(() => {
  mock.restore();
});

const created: Created = {
  source_id: SOURCE_ID,
  id: 43,
  tag: 'v1.2.4',
  name: 'v1.2.4',
  url: 'https://github.com/acme/zone/releases/tag/v1.2.4',
  published_at: '2026-11-13T10:00:00Z',
};

const SECOND: SourceOptions = {
  source_id: 'second-source',
  repository: 'acme/console',
  repository_url: 'https://github.com/acme/console',
  default_branch: 'main',
  latest_tag: null,
  next_tag: 'v0.1.0',
  previous_tags: [],
  lines: [],
};

const onClose = mock();
const onCreated = mock();

function renderWizard(sourceId?: string) {
  const { CreateReleaseWizard } = wizard;
  return render(
    <MemoryRouter>
      <CreateReleaseWizard isOpen onClose={onClose} onCreated={onCreated} sourceId={sourceId} />
    </MemoryRouter>
  );
}

function tagInput(): HTMLInputElement {
  return screen.getByLabelText('Tag') as HTMLInputElement;
}

async function reviewStep() {
  fireEvent.click(screen.getByRole('button', { name: wizard.REVIEW_LABEL }));
  await screen.findByRole('heading', { name: 'Notes' });
}

describe('CreateReleaseWizard', () => {
  beforeEach(() => {
    for (const fake of [mockPreview, mockCreate, mockReset, mockReload, onClose, onCreated]) {
      fake.mockReset();
    }
    sources = options.sources;
    optionsLoading = false;
    optionsError = null;
    opened.length = 0;
  });

  it('reads the options only while open', () => {
    const { CreateReleaseWizard } = wizard;
    render(
      <MemoryRouter>
        <CreateReleaseWizard isOpen={false} onClose={onClose} onCreated={onCreated} />
      </MemoryRouter>
    );
    expect(opened.every((open) => !open)).toBe(true);
  });

  it('prefills the next tag and lists the earlier ones', () => {
    renderWizard();
    expect(tagInput().value).toBe('v1.2.4');
    expect(screen.getByText(wizard.previousTags(['v1.2.3', 'v1.2.2']))).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Repository' })).toHaveTextContent('acme/zone');
  });

  it('starts on the repository the page is filtered to', () => {
    sources = [...options.sources, SECOND];
    renderWizard(SECOND.source_id);
    expect(tagInput().value).toBe('v0.1.0');
    expect(screen.getByText(wizard.NO_PREVIOUS_TAGS)).toBeInTheDocument();
  });

  it('refuses a malformed or existing tag before asking the server', () => {
    renderWizard();
    fireEvent.change(tagInput(), { target: { value: 'v1 2' } });
    expect(screen.getByText(TagProblem.Whitespace)).toBeInTheDocument();
    expect(tagInput()).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByRole('button', { name: wizard.REVIEW_LABEL })).toBeDisabled();

    fireEvent.change(tagInput(), { target: { value: 'v1.2.3' } });
    expect(screen.getByText('v1.2.3 already exists')).toBeInTheDocument();
    expect(mockPreview).not.toHaveBeenCalled();
  });

  it('previews against the latest tag, then publishes with the digest', async () => {
    mockPreview.mockResolvedValue({
      ...preview,
      pulls: [{ number: 101, url: 'https://github.com/acme/zone/pull/101', title: 'Add pages' }],
    });
    mockCreate.mockResolvedValue(created);
    renderWizard();

    expect(screen.queryByRole('checkbox', { name: 'Pre-release' })).toBeNull();
    expect(screen.getByText(wizard.STABLE_TAG)).toBeInTheDocument();
    await reviewStep();

    expect(mockPreview).toHaveBeenCalledWith({
      source_id: SOURCE_ID,
      tag: 'v1.2.4',
      expected_latest_tag: 'v1.2.3',
    });
    expect(screen.getByText('bbbbbbb')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /#101/ })).toHaveAttribute(
      'href',
      'https://github.com/acme/zone/pull/101'
    );
    expect(screen.getByRole('heading', { name: 'What changed' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(created));
    expect(mockCreate).toHaveBeenCalledWith({
      source_id: SOURCE_ID,
      tag: 'v1.2.4',
      expected_latest_tag: 'v1.2.3',
      prerelease: false,
      digest: preview.digest,
    });
    expect(onClose).toHaveBeenCalled();
  });

  it('publishes a pre-release tag as a pre-release', async () => {
    const candidate = { ...preview, tag: 'v1.3.0-rc.1', name: 'v1.3.0-rc.1', prerelease: true };
    mockPreview.mockResolvedValue(candidate);
    mockCreate.mockResolvedValue({ ...created, tag: candidate.tag });
    renderWizard();

    fireEvent.change(tagInput(), { target: { value: 'v1.3.0-rc.1' } });
    expect(screen.getByText(wizard.PRERELEASE_TAG)).toBeInTheDocument();
    expect(tagInput().getAttribute('aria-describedby')).toContain('release-tag-kind');
    await reviewStep();

    const facts = screen.getByText('Pre-release').nextElementSibling;
    expect(facts).toHaveTextContent('Yes');
    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
    expect(mockCreate).toHaveBeenCalledWith({
      source_id: SOURCE_ID,
      tag: 'v1.3.0-rc.1',
      expected_latest_tag: 'v1.2.3',
      prerelease: true,
      digest: preview.digest,
    });
  });

  it('says nothing about the release kind of a tag it cannot read', () => {
    renderWizard();
    fireEvent.change(tagInput(), { target: { value: 'nightly' } });
    expect(screen.getByText(TagProblem.Version)).toBeInTheDocument();
    expect(screen.queryByText(wizard.STABLE_TAG)).toBeNull();
    expect(screen.queryByText(wizard.PRERELEASE_TAG)).toBeNull();
  });

  describe('with a repository tagged by prefix', () => {
    beforeEach(() => {
      sources = [prefixedSource];
    });

    it('offers each line and starts on the busiest one', () => {
      renderWizard();
      expect(screen.getByRole('combobox', { name: wizard.LINE_LABEL })).toHaveTextContent(
        'abnegate-vcs'
      );
      expect(selectOptionValues(wizard.LINE_LABEL)).toHaveLength(2);
      expect(tagInput().value).toBe('abnegate-vcs/v0.2.2');
      expect(
        screen.getByText(
          wizard.previousTags([
            'abnegate-vcs/v0.3.0-rc.1',
            'abnegate-vcs/v0.2.1',
            'abnegate-vcs/v0.2.0',
          ])
        )
      ).toBeInTheDocument();
    });

    it("prefills the chosen line's next tag and previews against its latest", async () => {
      mockPreview.mockResolvedValue({ ...preview, tag: 'abnegate-llm/v1.0.1' });
      renderWizard();

      chooseSelect(wizard.LINE_LABEL, 'abnegate-llm');

      expect(tagInput().value).toBe('abnegate-llm/v1.0.1');
      expect(screen.getByText(wizard.previousTags(['abnegate-llm/v1.0.0']))).toBeInTheDocument();
      await reviewStep();
      expect(mockPreview).toHaveBeenCalledWith({
        source_id: PREFIXED_SOURCE_ID,
        tag: 'abnegate-llm/v1.0.1',
        expected_latest_tag: 'abnegate-llm/v1.0.0',
      });
    });

    it("expects the latest tag of the typed tag's own line", async () => {
      mockPreview.mockResolvedValue({ ...preview, tag: 'abnegate-llm/v1.1.0' });
      renderWizard();

      fireEvent.change(tagInput(), { target: { value: 'abnegate-llm/v1.1.0' } });
      await reviewStep();

      expect(mockPreview).toHaveBeenCalledWith({
        source_id: PREFIXED_SOURCE_ID,
        tag: 'abnegate-llm/v1.1.0',
        expected_latest_tag: 'abnegate-llm/v1.0.0',
      });
    });

    it('expects no latest tag for a new line', async () => {
      mockPreview.mockResolvedValue({ ...preview, tag: 'abnegate-cli/v0.1.0' });
      renderWizard();

      fireEvent.change(tagInput(), { target: { value: 'abnegate-cli/v0.1.0' } });
      await reviewStep();

      expect(mockPreview).toHaveBeenCalledWith({
        source_id: PREFIXED_SOURCE_ID,
        tag: 'abnegate-cli/v0.1.0',
        expected_latest_tag: null,
      });
    });

    it('refuses a tag its own line already has', () => {
      renderWizard();
      fireEvent.change(tagInput(), { target: { value: 'abnegate-llm/v1.0.0' } });
      expect(screen.getByText('abnegate-llm/v1.0.0 already exists')).toBeInTheDocument();
    });

    it('keeps an edited tag while the line stays the same', () => {
      renderWizard();
      fireEvent.change(tagInput(), { target: { value: 'abnegate-vcs/v0.3.0' } });
      chooseSelect(wizard.LINE_LABEL, 'abnegate-llm');
      expect(tagInput().value).toBe('abnegate-llm/v1.0.1');
      chooseSelect(wizard.LINE_LABEL, 'abnegate-vcs');
      expect(tagInput().value).toBe('abnegate-vcs/v0.3.0');
    });
  });

  it('offers no line choice when the repository has one line', () => {
    renderWizard();
    expect(screen.queryByRole('combobox', { name: wizard.LINE_LABEL })).toBeNull();
  });

  it('puts focus on the tag once the options load', () => {
    renderWizard();
    expect(document.activeElement).toBe(tagInput());
  });

  it('publishes once however fast Publish is pressed', async () => {
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockReturnValue(new Promise(() => {}));
    renderWizard();
    await reviewStep();
    const publish = screen.getByRole('button', { name: wizard.PUBLISH });

    act(() => {
      fireEvent.click(publish);
      fireEvent.click(publish);
    });

    await waitFor(() => expect(mockCreate).toHaveBeenCalledTimes(1));
  });

  it('says when nothing was merged since the last tag', async () => {
    mockPreview.mockResolvedValue(preview);
    renderWizard();
    await reviewStep();
    expect(screen.getByText(wizard.NO_INCLUDED_PULLS)).toBeInTheDocument();
  });

  it('offers to reload the options when a publish is refused', async () => {
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockRejectedValue(
      new ReleaseRefusal({ error: 'The preview is stale', code: 'preview_stale' })
    );
    mockReload.mockResolvedValue(undefined);
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    expect(await screen.findByRole('alert')).toHaveTextContent('The preview is stale');
    expect(onCreated).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: wizard.RELOAD_OPTIONS }));
    await waitFor(() => expect(mockReload).toHaveBeenCalled());
    expect(mockReset).toHaveBeenCalled();
    expect(await screen.findByLabelText('Tag')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('offers to reload the options when a preview is refused', async () => {
    mockPreview.mockRejectedValue(
      new ReleaseRefusal({ error: 'v1.2.4 already exists', code: 'tag_exists', tag: 'v1.2.4' })
    );
    renderWizard();
    fireEvent.click(screen.getByRole('button', { name: wizard.REVIEW_LABEL }));

    expect(await screen.findByRole('alert')).toHaveTextContent('v1.2.4 already exists');
    expect(screen.getByRole('button', { name: wizard.RELOAD_OPTIONS })).toBeInTheDocument();
    expect(screen.getByLabelText('Tag')).toBeInTheDocument();
  });

  it('lets the tag be fixed when a preview finds it is not newer', async () => {
    const message = 'v1.2.4 is not newer than the latest tag, v1.3.0. Choose a newer tag.';
    mockPreview.mockRejectedValueOnce(
      new ReleaseRefusal({
        error: message,
        code: 'tag_not_newer',
        tag: 'v1.2.4',
        latest_tag: 'v1.3.0',
      })
    );
    renderWizard();
    fireEvent.click(screen.getByRole('button', { name: wizard.REVIEW_LABEL }));

    expect(await screen.findByRole('alert')).toHaveTextContent(message);
    fireEvent.change(tagInput(), { target: { value: 'v1.3.1' } });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('button', { name: wizard.REVIEW_LABEL })).toBeEnabled();
  });

  it('returns to the tag when a publish finds a release already uses it', async () => {
    const message = 'Release 91 already uses the tag v1.2.4. Choose another tag.';
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockRejectedValue(
      new ReleaseRefusal({ error: message, code: 'release_exists', tag: 'v1.2.4', release_id: 91 })
    );
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));

    expect(await screen.findByRole('alert')).toHaveTextContent(message);
    expect(tagInput().value).toBe('v1.2.4');
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('explains a release that stopped after writing its tag', async () => {
    mockPreview.mockResolvedValue(preview);
    const reconciliation: Reconciliation = {
      error: 'The release was only partly created and needs reconciling on GitHub',
      code: 'release_manual_reconciliation_required',
      tag: 'v1.2.4',
      tag_object: 'e'.repeat(40),
      reference_created: null,
      release_id: null,
      release_exists: true,
      marker: '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b',
    };
    mockCreate.mockRejectedValue(new ReleaseRefusal(reconciliation));
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(RECONCILIATION_NEEDED);
    const facts = within(alert)
      .getAllByRole('listitem')
      .map((item) => item.textContent);
    expect(facts).toEqual(reconciliationFacts(reconciliation));
    expect(within(alert).getByRole('link', { name: OPEN_RELEASES })).toHaveAttribute(
      'href',
      'https://github.com/acme/zone/releases'
    );
    expect(within(alert).getByRole('button', { name: wizard.RELOAD_OPTIONS })).toBeInTheDocument();
  });

  it('stops a second publish once the tag needs reconciling, since it can only find the tag taken', async () => {
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockRejectedValue(
      new ReleaseRefusal({
        error: 'The release was only partly created and needs reconciling on GitHub',
        code: 'release_manual_reconciliation_required',
        tag: 'v1.2.4',
        tag_object: 'e'.repeat(40),
        reference_created: true,
        release_id: null,
        release_exists: false,
        marker: '0b9f1c52-6d4e-4a8b-9f2e-1c3d5e7f9a0b',
      })
    );
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    const alert = await screen.findByRole('alert');

    expect(screen.getByRole('button', { name: wizard.PUBLISH })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    expect(mockCreate).toHaveBeenCalledTimes(1);
    expect(within(alert).getByRole('button', { name: wizard.RELOAD_OPTIONS })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Previous' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(tagInput()).toBeInTheDocument());
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('asks for GitHub to be checked when a publish stopped without an answer', async () => {
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockRejectedValue(
      new ApiError('Failed to create release: stopped', HttpStatus.InternalServerError)
    );
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(wizard.CREATE_OUTCOME_UNKNOWN);
    expect(within(alert).getByRole('link', { name: OPEN_RELEASES })).toHaveAttribute(
      'href',
      'https://github.com/acme/zone/releases'
    );
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('reports a failed preview as it is, even a 500', async () => {
    mockPreview.mockRejectedValue(
      new ApiError('Failed to preview release: Could not read the source.', 500)
    );
    renderWizard();
    fireEvent.click(screen.getByRole('button', { name: wizard.REVIEW_LABEL }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('Failed to preview release: Could not read the source.');
    expect(within(alert).queryByRole('link')).toBeNull();
  });

  it.each([
    [
      'a missing permission',
      new ApiError('Failed to create release: Missing permission: releases:create', 403),
    ],
    [
      'a pre-release that disagrees with its tag',
      new ApiError(
        'Failed to create release: prerelease must be true exactly when the tag is a prerelease such as v1.2.3-rc.1.',
        400
      ),
    ],
  ])('shows %s as the server put it', async (_case, error) => {
    mockPreview.mockResolvedValue(preview);
    mockCreate.mockRejectedValue(error);
    renderWizard();
    await reviewStep();

    fireEvent.click(screen.getByRole('button', { name: wizard.PUBLISH }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent(error.message);
    expect(within(alert).queryByRole('button', { name: wizard.RELOAD_OPTIONS })).toBeNull();
  });

  it('shows a plain failure without the reload offer', async () => {
    mockPreview.mockRejectedValue(new Error('GitHub is down'));
    renderWizard();
    fireEvent.click(screen.getByRole('button', { name: wizard.REVIEW_LABEL }));
    expect(await screen.findByRole('alert')).toHaveTextContent('GitHub is down');
    expect(screen.queryByRole('button', { name: wizard.RELOAD_OPTIONS })).toBeNull();
  });

  it('links to the sources settings when there is nothing to release from', () => {
    sources = [];
    renderWizard();
    expect(screen.getByText(wizard.NO_SOURCES)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: wizard.CONNECT_SOURCE })).toHaveAttribute(
      'href',
      '/settings/workspace/sources'
    );
    expect(screen.getByRole('button', { name: wizard.REVIEW_LABEL })).toBeDisabled();
  });

  it('waits for the options to load', () => {
    sources = [];
    optionsLoading = true;
    renderWizard();
    expect(screen.getByRole('status')).toHaveTextContent(wizard.LOADING_OPTIONS);
  });

  it('offers a reload when the options failed to load', () => {
    sources = [];
    optionsError = 'Failed to load release options';
    renderWizard();
    expect(screen.getByRole('alert')).toHaveTextContent(optionsError);
    fireEvent.click(screen.getByRole('button', { name: wizard.RELOAD_OPTIONS }));
    expect(mockReload).toHaveBeenCalled();
  });
});

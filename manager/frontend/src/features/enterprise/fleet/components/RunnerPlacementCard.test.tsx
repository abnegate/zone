import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { FleetRefusal } from '../FleetRefusal';
import {
  COMMUNITY_ORGANIZATION_LABELS,
  LAPSED_ORGANIZATION_LABELS,
  ORGANIZATION_ID,
  ORGANIZATION_LABELS,
  UNMATCHED_WORKSPACE_LABELS,
  WORKSPACE_ID,
  WORKSPACE_LABELS,
} from '../fixtures';
import {
  AVAILABLE_LABEL,
  CHATS_IGNORE_LABELS,
  EFFECTIVE_LABEL,
  ENTRY_LABEL,
  inheritedAlready,
  invalidLabels,
  matchingText,
  noMatchWarning,
  ORGANIZATION_REQUIRED_LABEL,
  PLACEMENT_LAPSED,
  PLACEMENT_TITLE,
  REQUIRED_LABEL,
  SAVE_LABELS,
  WORKSPACE_REQUIRED_LABEL,
} from '../messages';
import {
  fromOrganization,
  fromWorkspace,
  organizationPlacement,
  type Placement,
  type PlacementScope,
  workspacePlacement,
} from '../placement';
import { FleetCode } from '../schemas';

const labels = mock();
const updateLabels = mock();

mock.module('../api', () => ({
  fleetApi: {
    policy: mock(),
    updatePolicy: mock(),
    runners: mock(),
    updateRunner: mock(),
    removeRunner: mock(),
    labels,
    updateLabels,
  },
}));

let RunnerPlacementCard: typeof import('./RunnerPlacementCard').RunnerPlacementCard;

beforeAll(async () => {
  ({ RunnerPlacementCard } = await import('./RunnerPlacementCard'));
});

afterAll(() => {
  mock.restore();
});

const ORGANIZATION = organizationPlacement(ORGANIZATION_ID);

const WORKSPACE = workspacePlacement(ORGANIZATION_ID, WORKSPACE_ID);

function renderCard(scope: PlacementScope) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <RunnerPlacementCard scope={scope} />
    </QueryClientProvider>
  );
}

async function loaded(scope: PlacementScope, placement: Placement) {
  labels.mockResolvedValue(placement);
  renderCard(scope);
  await screen.findByRole('group', { name: REQUIRED_LABEL });
}

function card(): HTMLElement {
  return screen.getByRole('region', { name: PLACEMENT_TITLE });
}

function chips(name: string): string[] {
  return within(screen.getByRole('list', { name }))
    .getAllByRole('listitem')
    .map((item) => item.textContent?.replace('×', '') ?? '');
}

function type(text: string) {
  fireEvent.change(screen.getByRole('textbox', { name: ENTRY_LABEL }), {
    target: { value: text },
  });
}

describe('RunnerPlacementCard', () => {
  beforeEach(() => {
    labels.mockReset();
    updateLabels.mockReset();
  });

  describe('in an organization', () => {
    it('says unattended work needs the labels and chats ignore them', async () => {
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      expect(within(card()).getByText(CHATS_IGNORE_LABELS)).toBeInTheDocument();
      expect(chips(REQUIRED_LABEL)).toEqual(['region=eu']);
      expect(screen.getByText(matchingText(2))).toBeInTheDocument();
      expect(labels).toHaveBeenCalledWith(ORGANIZATION);
    });

    it('offers the labels runners carry that are not yet required, with their counts', async () => {
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      const offered = within(screen.getByRole('list', { name: AVAILABLE_LABEL }));
      expect(offered.getAllByRole('button').map((button) => button.textContent)).toEqual([
        'gpu1 runner',
        'region=us1 runner',
      ]);
    });

    it('saves a picked label and a typed one together', async () => {
      updateLabels.mockResolvedValue(
        fromOrganization({ ...ORGANIZATION_LABELS, labels: ['gpu', 'region=eu', 'zone=a'] })
      );
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      fireEvent.click(screen.getByRole('button', { name: 'Require gpu' }));
      type('zone=a');
      fireEvent.click(screen.getByRole('button', { name: SAVE_LABELS }));

      await waitFor(() =>
        expect(updateLabels).toHaveBeenCalledWith(ORGANIZATION, {
          labels: ['gpu', 'region=eu', 'zone=a'],
        })
      );
    });

    it('clears the requirement by removing every label', async () => {
      updateLabels.mockResolvedValue(fromOrganization({ ...ORGANIZATION_LABELS, labels: [] }));
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      fireEvent.click(screen.getByRole('button', { name: 'Remove region=eu' }));
      fireEvent.click(screen.getByRole('button', { name: SAVE_LABELS }));

      await waitFor(() => expect(updateLabels).toHaveBeenCalledWith(ORGANIZATION, { labels: [] }));
    });

    it('refuses a label the server would refuse before sending it', async () => {
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      type('GPU');
      fireEvent.click(screen.getByRole('button', { name: SAVE_LABELS }));

      expect(await screen.findByRole('alert')).toHaveTextContent(invalidLabels(['GPU']));
      expect(screen.getByRole('textbox', { name: ENTRY_LABEL })).toHaveAttribute(
        'aria-invalid',
        'true'
      );
      expect(updateLabels).not.toHaveBeenCalled();
    });

    it('shows the server wording when it refuses a label', async () => {
      updateLabels.mockRejectedValue(
        new FleetRefusal(
          { error: '"x" is not a valid runner label.', code: FleetCode.LabelInvalid },
          400
        )
      );
      await loaded(ORGANIZATION, fromOrganization(ORGANIZATION_LABELS));

      fireEvent.click(screen.getByRole('button', { name: 'Require gpu' }));
      fireEvent.click(screen.getByRole('button', { name: SAVE_LABELS }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        '"x" is not a valid runner label.'
      );
    });

    it('keeps a lapsed requirement read-only and says it is still enforced', async () => {
      await loaded(ORGANIZATION, fromOrganization(LAPSED_ORGANIZATION_LABELS));

      expect(screen.getByText(PLACEMENT_LAPSED)).toBeInTheDocument();
      expect(screen.queryByRole('textbox', { name: ENTRY_LABEL })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABELS })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Remove region=eu' })).not.toBeInTheDocument();
    });

    it('names the permission a member needs to change the labels', async () => {
      await loaded(ORGANIZATION, fromOrganization({ ...ORGANIZATION_LABELS, editable: false }));

      expect(
        screen.getByText(
          'Changing the runner labels needs the policies:update permission in this organization.'
        )
      ).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABELS })).not.toBeInTheDocument();
    });

    it('stays out of the way on Community', async () => {
      labels.mockResolvedValue(fromOrganization(COMMUNITY_ORGANIZATION_LABELS));
      renderCard(ORGANIZATION);

      await waitFor(() =>
        expect(screen.queryByRole('region', { name: PLACEMENT_TITLE })).not.toBeInTheDocument()
      );
    });
  });

  describe('in a workspace', () => {
    it('shows the organization labels locked, its own labels, and what is in effect', async () => {
      await loaded(WORKSPACE, fromWorkspace(WORKSPACE_LABELS));

      expect(chips(ORGANIZATION_REQUIRED_LABEL)).toEqual(['region=eu']);
      expect(
        within(screen.getByRole('list', { name: ORGANIZATION_REQUIRED_LABEL })).queryByRole(
          'button'
        )
      ).not.toBeInTheDocument();
      expect(chips(WORKSPACE_REQUIRED_LABEL)).toEqual(['gpu']);
      expect(screen.getByRole('group', { name: REQUIRED_LABEL })).toHaveTextContent(
        `${EFFECTIVE_LABEL}gpu, region=eu`
      );
      expect(labels).toHaveBeenCalledWith(WORKSPACE);
    });

    it('names the permission a member needs to change the workspace labels', async () => {
      await loaded(WORKSPACE, fromWorkspace({ ...WORKSPACE_LABELS, editable: false }));

      expect(
        screen.getByText(
          "Changing this workspace's runner labels needs the workspace_policies:update permission in this workspace."
        )
      ).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: SAVE_LABELS })).not.toBeInTheDocument();
    });

    it('does not offer a label the organization already requires', async () => {
      await loaded(WORKSPACE, fromWorkspace(WORKSPACE_LABELS));

      const offered = within(screen.getByRole('list', { name: AVAILABLE_LABEL }));
      expect(offered.getAllByRole('button').map((button) => button.textContent)).toEqual([
        'region=us1 runner',
      ]);

      type('region=eu');
      fireEvent.click(screen.getByRole('button', { name: ENTRY_LABEL }));
      expect(await screen.findByRole('alert')).toHaveTextContent(inheritedAlready(['region=eu']));
    });

    it('saves only its own labels', async () => {
      updateLabels.mockResolvedValue(fromWorkspace(UNMATCHED_WORKSPACE_LABELS));
      await loaded(WORKSPACE, fromWorkspace(WORKSPACE_LABELS));

      fireEvent.click(screen.getByRole('button', { name: 'Remove gpu' }));
      fireEvent.click(screen.getByRole('button', { name: 'Require region=us' }));
      fireEvent.click(screen.getByRole('button', { name: SAVE_LABELS }));

      await waitFor(() =>
        expect(updateLabels).toHaveBeenCalledWith(WORKSPACE, { labels: ['region=us'] })
      );
    });

    it('warns when no connected runner carries every label in effect', async () => {
      await loaded(WORKSPACE, fromWorkspace(UNMATCHED_WORKSPACE_LABELS));

      expect(
        screen.getByText(noMatchWarning(UNMATCHED_WORKSPACE_LABELS.effective))
      ).toBeInTheDocument();
    });
  });
});

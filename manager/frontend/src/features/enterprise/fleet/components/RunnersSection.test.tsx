import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { AdministersServerProvider } from '../../components/AdministersServer';
import { FleetRefusal } from '../FleetRefusal';
import {
  ACTIVE_POLICY,
  ACTIVE_RUNNERS,
  COMMUNITY_POLICY,
  COMMUNITY_RUNNERS,
  CORDONED_RUNNER,
  CURRENT_RUNNER,
  LAPSED_POLICY,
  LAPSED_RUNNERS,
  NO_RUNNER_LEFT,
  OFFLINE_RUNNER,
  OPEN_POLICY,
  OUTDATED_RUNNER,
  REVOKED_RUNNER,
  UNVERIFIED_POLICY,
  UNVERIFIED_RUNNERS,
} from '../fixtures';
import {
  CLEAR_MINIMUM,
  CURRENT_BADGE,
  CURRENT_WARNING,
  consequence,
  LAPSED_NOTICE,
  MINIMUM_LABEL,
  NO_MINIMUM,
  NO_RUNNER_LEFT_CONFIRM,
  NO_RUNNER_LEFT_TITLE,
  NOT_ENFORCED,
  OFFLINE,
  OUTDATED_BADGE,
  RunnerAction,
  refusalMessage,
  SAVE_MINIMUM,
  SERVING,
  UNVERIFIED_NOTICE,
  UPGRADE_NOTICE,
  workReason,
} from '../messages';
import { FleetCode, RunnerState } from '../schemas';
import type { FleetPolicyResponse, RunnersResponse } from '../types';

const policy = mock();
const updatePolicy = mock();
const runners = mock();
const updateRunner = mock();
const removeRunner = mock();

mock.module('../api', () => ({
  fleetApi: {
    policy,
    updatePolicy,
    runners,
    updateRunner,
    removeRunner,
    labels: mock(),
    updateLabels: mock(),
  },
}));

let RunnersSection: typeof import('./RunnersSection').RunnersSection;

beforeAll(async () => {
  ({ RunnersSection } = await import('./RunnersSection'));
});

afterAll(() => {
  mock.restore();
});

function renderSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <AdministersServerProvider value>
          <RunnersSection />
        </AdministersServerProvider>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(fleet: FleetPolicyResponse, listed: RunnersResponse) {
  policy.mockResolvedValue(fleet);
  runners.mockResolvedValue(listed);
  renderSection();
  await screen.findByRole('heading', { name: 'Runners', level: 2 });
}

function row(name: string): HTMLElement {
  return screen.getByRole('row', { name });
}

function dialog(): HTMLElement {
  return screen.getByRole('dialog');
}

describe('RunnersSection', () => {
  beforeEach(() => {
    for (const call of [policy, updatePolicy, runners, updateRunner, removeRunner]) {
      call.mockReset();
    }
  });

  describe('states', () => {
    it('shows a loading state until the fleet arrives', () => {
      policy.mockImplementation(() => new Promise(() => {}));
      runners.mockImplementation(() => new Promise(() => {}));
      renderSection();

      expect(screen.getByText('Loading the runners...')).toBeInTheDocument();
    });

    it('offers a retry when the fleet fails to load', async () => {
      policy.mockResolvedValue(ACTIVE_POLICY);
      runners
        .mockRejectedValueOnce(new ApiError('Failed to load the runners: Not found', 404))
        .mockResolvedValueOnce(ACTIVE_RUNNERS);
      renderSection();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load the runners: Not found');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('row', { name: new RegExp(CURRENT_RUNNER.name) });
    });

    it('lists the fleet on Community read-only, offers the upgrade and says nothing is enforced', async () => {
      await loaded(COMMUNITY_POLICY, COMMUNITY_RUNNERS);

      expect(screen.getByText(UPGRADE_NOTICE)).toBeInTheDocument();
      expect(screen.getByText(NOT_ENFORCED)).toBeInTheDocument();
      expect(screen.getByText(NO_MINIMUM)).toBeInTheDocument();
      expect(screen.queryByRole('textbox', { name: MINIMUM_LABEL })).not.toBeInTheDocument();
      expect(row(CORDONED_RUNNER.name)).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Cordon / })).not.toBeInTheDocument();
      expect(screen.queryByRole('columnheader', { name: 'Actions' })).not.toBeInTheDocument();
    });

    it('keeps a lapsed fleet read-only and enforced', async () => {
      await loaded(LAPSED_POLICY, LAPSED_RUNNERS);

      expect(screen.getByText(LAPSED_NOTICE)).toBeInTheDocument();
      expect(screen.queryByText(NOT_ENFORCED)).not.toBeInTheDocument();
      expect(screen.getByText('1.4.0')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Revoke / })).not.toBeInTheDocument();
    });

    it('says a stored key that no longer verifies keeps the fleet enforced and read-only', async () => {
      await loaded(UNVERIFIED_POLICY, UNVERIFIED_RUNNERS);

      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(UNVERIFIED_NOTICE);
      expect(within(warning).getByRole('link')).toHaveAttribute('href', '/settings/server/license');
      expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
      expect(screen.queryByText(LAPSED_NOTICE)).not.toBeInTheDocument();
      expect(screen.queryByText(NOT_ENFORCED)).not.toBeInTheDocument();
      expect(screen.getByText('1.4.0')).toBeInTheDocument();
      expect(screen.queryByRole('textbox', { name: MINIMUM_LABEL })).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Revoke / })).not.toBeInTheDocument();
    });
  });

  describe('the runners table', () => {
    it('shows each runner with its version, labels, state, work and presence', async () => {
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      const current = row(CURRENT_RUNNER.name);
      expect(within(current).getByText(CURRENT_BADGE)).toBeInTheDocument();
      expect(within(current).getByText('1.4.2')).toBeInTheDocument();
      expect(
        within(current)
          .getAllByRole('listitem')
          .map((item) => item.textContent)
      ).toEqual(['gpu', 'region=eu']);
      expect(within(current).getByText('Active')).toBeInTheDocument();
      expect(within(current).getByText(SERVING)).toBeInTheDocument();
      expect(within(current).getByText('Connected')).toBeInTheDocument();
      expect(within(current).getByText('3')).toBeInTheDocument();

      const outdated = row(OUTDATED_RUNNER.name);
      expect(within(outdated).getByText(OUTDATED_BADGE)).toBeInTheDocument();
      expect(within(outdated).getByText(workReason(FleetCode.RunnerOutdated))).toBeInTheDocument();
      expect(within(outdated).queryByText(CURRENT_BADGE)).not.toBeInTheDocument();

      expect(
        within(row(CORDONED_RUNNER.name)).getByText(workReason(FleetCode.RunnerCordoned))
      ).toBeInTheDocument();
      const revoked = row(REVOKED_RUNNER.name);
      expect(within(revoked).getByText('Revoked')).toBeInTheDocument();
      expect(within(revoked).getByText(/^Last seen /)).toBeInTheDocument();

      const offline = row(OFFLINE_RUNNER.name);
      expect(within(offline).getByText(OFFLINE)).toBeInTheDocument();
      expect(within(offline).getByText('No labels')).toBeInTheDocument();
    });

    it('offers the moves each state allows, and Remove only for an offline runner', async () => {
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      const buttons = (name: string) =>
        within(row(name))
          .getAllByRole('button')
          .map((button) => button.textContent);

      expect(buttons(CURRENT_RUNNER.name)).toEqual(['Cordon', 'Revoke']);
      expect(buttons(CORDONED_RUNNER.name)).toEqual(['Activate', 'Revoke']);
      expect(buttons(REVOKED_RUNNER.name)).toEqual(['Activate', 'Remove']);
      expect(buttons(OFFLINE_RUNNER.name)).toEqual(['Cordon', 'Revoke', 'Remove']);
    });
  });

  describe('changing a runner', () => {
    it('cordons a runner after the admin confirms what it means', async () => {
      updateRunner.mockResolvedValue({ ...CURRENT_RUNNER, state: RunnerState.Cordoned });
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Cordon ${CURRENT_RUNNER.name}` }));
      expect(
        within(dialog()).getByText(consequence(RunnerAction.Cordon, CURRENT_RUNNER.name))
      ).toBeInTheDocument();
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Cordon runner' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(updateRunner).toHaveBeenCalledWith(CURRENT_RUNNER.id, {
        state: RunnerState.Cordoned,
      });
    });

    it('warns that a revoke stops live task runs and ends chats', async () => {
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Revoke ${CORDONED_RUNNER.name}` }));

      expect(within(dialog()).getByRole('heading')).toHaveTextContent(
        `Revoke ${CORDONED_RUNNER.name}?`
      );
      expect(dialog()).toHaveTextContent('Its live task runs stop and its chats end');
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(updateRunner).not.toHaveBeenCalled();
    });

    it('warns that revoking this server ends chats on it, and says nothing of it on cordon', async () => {
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Cordon ${CURRENT_RUNNER.name}` }));
      expect(within(dialog()).queryByText(CURRENT_WARNING)).not.toBeInTheDocument();
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));

      fireEvent.click(screen.getByRole('button', { name: `Revoke ${CURRENT_RUNNER.name}` }));
      expect(within(dialog()).getByText(CURRENT_WARNING)).toBeInTheDocument();
      expect(CURRENT_WARNING).toContain('Reload the page');
    });

    it('asks again when the change would leave no runner, and resends it confirmed', async () => {
      updateRunner
        .mockRejectedValueOnce(new FleetRefusal(NO_RUNNER_LEFT, 409))
        .mockResolvedValueOnce({ ...CURRENT_RUNNER, state: RunnerState.Revoked });
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Revoke ${CURRENT_RUNNER.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Revoke runner' }));

      await screen.findByRole('heading', { name: NO_RUNNER_LEFT_TITLE });
      fireEvent.click(within(dialog()).getByRole('button', { name: NO_RUNNER_LEFT_CONFIRM }));

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(updateRunner.mock.calls).toEqual([
        [CURRENT_RUNNER.id, { state: RunnerState.Revoked }],
        [CURRENT_RUNNER.id, { state: RunnerState.Revoked, confirm: true }],
      ]);
    });

    it('shows why a change was refused inside its dialog', async () => {
      updateRunner.mockRejectedValue(
        new FleetRefusal(
          { error: 'Requires Zone Enterprise', code: FleetCode.LicenseRequired },
          403
        )
      );
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Activate ${REVOKED_RUNNER.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Activate runner' }));

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        'Changing the fleet policy needs a Zone Enterprise license'
      );
    });

    it('removes an offline runner', async () => {
      removeRunner.mockResolvedValue(undefined);
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Remove ${OFFLINE_RUNNER.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Remove runner' }));

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(removeRunner).toHaveBeenCalledWith(OFFLINE_RUNNER.id);
    });

    it('explains why an offline runner that still holds live work is kept', async () => {
      removeRunner.mockRejectedValue(
        new FleetRefusal(
          { error: 'This runner still holds live work.', code: FleetCode.RunnerBusy },
          409
        )
      );
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: `Remove ${OFFLINE_RUNNER.name}` }));
      fireEvent.click(within(dialog()).getByRole('button', { name: 'Remove runner' }));

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        refusalMessage(FleetCode.RunnerBusy)
      );
    });
  });

  describe('the minimum version', () => {
    it('saves a new minimum', async () => {
      updatePolicy.mockResolvedValue({ ...ACTIVE_POLICY, minimum_version: '1.5.0' });
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      const input = screen.getByRole('textbox', { name: MINIMUM_LABEL });
      expect(input).toHaveValue('1.4.0');
      expect(screen.getByRole('button', { name: SAVE_MINIMUM })).toBeDisabled();
      fireEvent.change(input, { target: { value: ' 1.5.0 ' } });
      fireEvent.click(screen.getByRole('button', { name: SAVE_MINIMUM }));

      await waitFor(() => expect(updatePolicy).toHaveBeenCalledWith({ minimum_version: '1.5.0' }));
    });

    it('clears the minimum', async () => {
      updatePolicy.mockResolvedValue(OPEN_POLICY);
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.click(screen.getByRole('button', { name: CLEAR_MINIMUM }));

      await waitFor(() => expect(updatePolicy).toHaveBeenCalledWith({ minimum_version: null }));
      expect(await screen.findByText(NO_MINIMUM)).toBeInTheDocument();
    });

    it('shows the server wording for a version it refused', async () => {
      updatePolicy.mockRejectedValue(
        new FleetRefusal(
          { error: '"1.4" is not a valid Zone version.', code: FleetCode.VersionInvalid },
          400
        )
      );
      await loaded(OPEN_POLICY, ACTIVE_RUNNERS);

      const input = screen.getByRole('textbox', { name: MINIMUM_LABEL });
      fireEvent.change(input, { target: { value: '1.4' } });
      fireEvent.click(screen.getByRole('button', { name: SAVE_MINIMUM }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        '"1.4" is not a valid Zone version.'
      );
      expect(input).toHaveAttribute('aria-invalid', 'true');
    });

    it('confirms a minimum that would leave no runner, then resends it', async () => {
      updatePolicy
        .mockRejectedValueOnce(new FleetRefusal(NO_RUNNER_LEFT, 409))
        .mockResolvedValueOnce({ ...ACTIVE_POLICY, minimum_version: '9.0.0' });
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.change(screen.getByRole('textbox', { name: MINIMUM_LABEL }), {
        target: { value: '9.0.0' },
      });
      fireEvent.click(screen.getByRole('button', { name: SAVE_MINIMUM }));
      await screen.findByRole('heading', { name: NO_RUNNER_LEFT_TITLE });
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      fireEvent.click(within(dialog()).getByRole('button', { name: NO_RUNNER_LEFT_CONFIRM }));

      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      expect(updatePolicy.mock.calls).toEqual([
        [{ minimum_version: '9.0.0' }],
        [{ minimum_version: '9.0.0', confirm: true }],
      ]);
    });

    it('shows why a confirmed resend was refused once, inside its dialog', async () => {
      updatePolicy
        .mockRejectedValueOnce(new FleetRefusal(NO_RUNNER_LEFT, 409))
        .mockRejectedValueOnce(
          new FleetRefusal(
            { error: 'Requires Zone Enterprise', code: FleetCode.LicenseRequired },
            403
          )
        );
      await loaded(ACTIVE_POLICY, ACTIVE_RUNNERS);

      fireEvent.change(screen.getByRole('textbox', { name: MINIMUM_LABEL }), {
        target: { value: '9.0.0' },
      });
      fireEvent.click(screen.getByRole('button', { name: SAVE_MINIMUM }));
      await screen.findByRole('heading', { name: NO_RUNNER_LEFT_TITLE });
      fireEvent.click(within(dialog()).getByRole('button', { name: NO_RUNNER_LEFT_CONFIRM }));

      expect(await within(dialog()).findByRole('alert')).toHaveTextContent(
        'Changing the fleet policy needs a Zone Enterprise license'
      );
      expect(screen.getAllByRole('alert', { hidden: true })).toHaveLength(1);
    });
  });
});

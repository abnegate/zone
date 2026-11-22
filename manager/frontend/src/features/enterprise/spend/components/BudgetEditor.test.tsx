import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { clientModule } from '../../../../test/client';
import { chooseSelect, selectOptionValues } from '../../../../test/select';
import {
  ADA_ID,
  GRACE_ID,
  MEMBERS,
  ORGANIZATION_BUDGET,
  ORGANIZATION_ID,
  PERSON_BUDGET,
  PLATFORM_WORKSPACE_ID,
  RESEARCH_WORKSPACE_ID,
  WORKSPACE_BUDGET,
  WORKSPACES,
} from '../fixtures';
import {
  ADD_LABEL,
  CANCEL_LABEL,
  HARD_LABEL,
  HARD_LIMIT_NOTE,
  LIMIT_LABEL,
  LIMIT_TOO_LARGE,
  PERSON_PICKER_LABEL,
  SAVE_LABEL,
  SCOPE_LABEL,
  THRESHOLD_LABEL,
  WORKSPACE_PICKER_LABEL,
} from '../labels';
import { refusalMessage } from '../messages';
import { SpendRefusal } from '../SpendRefusal';
import { type Budget, SPEND_REFUSAL_CODES, SpendCode, type SpendRefusalCode } from '../schemas';

const createBudget = mock();
const updateBudget = mock();
const getWorkspaces = mock();
const getOrgMembers = mock();
const done = mock();

mock.module('../api', () => ({
  spendApi: {
    report: mock(),
    download: mock(),
    budgets: mock(),
    createBudget,
    updateBudget,
    deleteBudget: mock(),
  },
}));

mock.module('../../../../api/client', () => clientModule({ getWorkspaces, getOrgMembers }));

let BudgetEditor: typeof import('./BudgetEditor').BudgetEditor;
let hooks: typeof import('../hooks');

beforeAll(async () => {
  ({ BudgetEditor } = await import('./BudgetEditor'));
  hooks = await import('../hooks');
});

afterAll(() => {
  mock.restore();
});

interface HarnessProps {
  budget: Budget | null;
  editable: boolean;
}

function Harness({ budget, editable }: HarnessProps) {
  const targets = hooks.useSpendTargets(ORGANIZATION_ID, true);
  const mutations = hooks.useBudgetMutations(ORGANIZATION_ID);
  return (
    <BudgetEditor
      budget={budget}
      editable={editable}
      workspaces={targets.workspaces}
      people={targets.people}
      mutations={mutations}
      onDone={done}
    />
  );
}

function renderEditor(budget: Budget | null = null, editable = true) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <Harness budget={budget} editable={editable} />
    </QueryClientProvider>
  );
}

function type(label: string, value: string) {
  fireEvent.change(screen.getByLabelText(label), { target: { value } });
}

function submit(name = ADD_LABEL) {
  fireEvent.click(screen.getByRole('button', { name }));
}

async function sourcesLoaded() {
  await waitFor(() => expect(getWorkspaces).toHaveBeenCalledWith(ORGANIZATION_ID));
  await waitFor(() => expect(getOrgMembers).toHaveBeenCalledWith(ORGANIZATION_ID));
}

describe('BudgetEditor', () => {
  beforeEach(() => {
    for (const fn of [createBudget, updateBudget, getWorkspaces, getOrgMembers, done]) {
      fn.mockReset();
    }
    getWorkspaces.mockResolvedValue([...WORKSPACES].reverse());
    getOrgMembers.mockResolvedValue({ members: MEMBERS });
  });

  describe('pickers', () => {
    it('offers the workspaces of the organization, sorted by name', async () => {
      renderEditor();
      await sourcesLoaded();

      chooseSelect(SCOPE_LABEL, 'Workspace');

      await waitFor(() =>
        expect(selectOptionValues(WORKSPACE_PICKER_LABEL)).toEqual([
          PLATFORM_WORKSPACE_ID,
          RESEARCH_WORKSPACE_ID,
        ])
      );
    });

    it('offers the members of the organization by name or email', async () => {
      renderEditor();
      await sourcesLoaded();

      chooseSelect(SCOPE_LABEL, 'Person');
      await waitFor(() =>
        expect(selectOptionValues(PERSON_PICKER_LABEL)).toEqual([ADA_ID, GRACE_ID])
      );
      chooseSelect(PERSON_PICKER_LABEL, 'grace@example.com');
      type(LIMIT_LABEL, '25');
      submit();

      await waitFor(() =>
        expect(createBudget).toHaveBeenCalledWith(ORGANIZATION_ID, {
          scope: 'user',
          user_id: GRACE_ID,
          limit_microdollars: 25_000_000,
          threshold_percent: 80,
          hard: false,
        })
      );
    });
  });

  describe('validation', () => {
    it('refuses a missing target, a zero limit and a threshold outside 1 to 100', async () => {
      renderEditor();
      await sourcesLoaded();

      chooseSelect(SCOPE_LABEL, 'Workspace');
      type(LIMIT_LABEL, '0');
      type(THRESHOLD_LABEL, '101');
      submit();

      expect(screen.getByText(refusalMessage(SpendCode.ScopeInvalid))).toBeInTheDocument();
      expect(screen.getByText(refusalMessage(SpendCode.LimitInvalid))).toBeInTheDocument();
      expect(screen.getByText(refusalMessage(SpendCode.ThresholdInvalid))).toBeInTheDocument();
      expect(createBudget).not.toHaveBeenCalled();
    });

    it('says a limit is too large instead of asking for one above $0', async () => {
      renderEditor();
      await sourcesLoaded();

      type(LIMIT_LABEL, '9007199255');
      submit();

      expect(screen.getByText(LIMIT_TOO_LARGE)).toBeInTheDocument();
      expect(screen.queryByText(refusalMessage(SpendCode.LimitInvalid))).not.toBeInTheDocument();
      expect(createBudget).not.toHaveBeenCalled();
    });

    it('waits for a submit before showing errors', async () => {
      renderEditor();
      await sourcesLoaded();

      type(LIMIT_LABEL, 'ten');

      expect(screen.queryByText(refusalMessage(SpendCode.LimitInvalid))).not.toBeInTheDocument();
    });

    it('adds a hard organization budget and clears the form', async () => {
      createBudget.mockResolvedValue(ORGANIZATION_BUDGET);
      renderEditor();
      await sourcesLoaded();

      expect(screen.getByText(HARD_LIMIT_NOTE)).toBeInTheDocument();
      type(LIMIT_LABEL, '$1,250.50');
      type(THRESHOLD_LABEL, '90');
      fireEvent.click(screen.getByRole('checkbox', { name: HARD_LABEL }));
      submit();

      await waitFor(() =>
        expect(createBudget).toHaveBeenCalledWith(ORGANIZATION_ID, {
          scope: 'organization',
          limit_microdollars: 1_250_500_000,
          threshold_percent: 90,
          hard: true,
        })
      );
      await waitFor(() => expect(screen.getByLabelText(LIMIT_LABEL)).toHaveValue(''));
      expect(done).toHaveBeenCalledTimes(1);
    });
  });

  describe('refusals', () => {
    const fields: Partial<Record<SpendRefusalCode, string>> = {
      [SpendCode.LimitInvalid]: LIMIT_LABEL,
      [SpendCode.ThresholdInvalid]: THRESHOLD_LABEL,
    };

    for (const code of SPEND_REFUSAL_CODES) {
      it(`shows ${code} in Zone's words`, async () => {
        createBudget.mockRejectedValue(new SpendRefusal({ error: 'raw', code }, 400));
        renderEditor();
        await sourcesLoaded();

        type(LIMIT_LABEL, '10');
        submit();

        const message = await screen.findByText(refusalMessage(code));
        expect(message).toBeInTheDocument();
        expect(screen.queryByText('raw')).not.toBeInTheDocument();
        const label = fields[code];
        if (label) {
          expect(screen.getByLabelText(label)).toHaveAttribute('aria-invalid', 'true');
        }
      });
    }

    it('clears a refusal once the budget is edited again', async () => {
      createBudget.mockRejectedValue(
        new SpendRefusal({ error: 'raw', code: SpendCode.BudgetExists }, 409)
      );
      renderEditor();
      await sourcesLoaded();

      type(LIMIT_LABEL, '10');
      submit();
      await screen.findByText(refusalMessage(SpendCode.BudgetExists));
      type(LIMIT_LABEL, '11');

      await waitFor(() =>
        expect(screen.queryByText(refusalMessage(SpendCode.BudgetExists))).not.toBeInTheDocument()
      );
    });
  });

  describe('editing', () => {
    it('fixes the target and sends only the changed fields', async () => {
      updateBudget.mockResolvedValue(WORKSPACE_BUDGET);
      renderEditor(WORKSPACE_BUDGET);
      await sourcesLoaded();

      expect(screen.queryByRole('combobox', { name: SCOPE_LABEL })).not.toBeInTheDocument();
      expect(screen.getByText('Platform')).toBeInTheDocument();
      expect(screen.getByLabelText(LIMIT_LABEL)).toHaveValue('35');
      expect(screen.getByRole('checkbox', { name: HARD_LABEL })).toBeChecked();
      fireEvent.click(screen.getByRole('checkbox', { name: HARD_LABEL }));
      submit(SAVE_LABEL);

      await waitFor(() =>
        expect(updateBudget).toHaveBeenCalledWith(ORGANIZATION_ID, WORKSPACE_BUDGET.id, {
          hard: false,
        })
      );
      await waitFor(() => expect(done).toHaveBeenCalledTimes(1));
    });

    it('closes without a request when nothing changed', async () => {
      renderEditor(PERSON_BUDGET);
      await sourcesLoaded();

      submit(SAVE_LABEL);

      expect(updateBudget).not.toHaveBeenCalled();
      expect(done).toHaveBeenCalledTimes(1);
    });

    it('cancels an edit', async () => {
      renderEditor(PERSON_BUDGET);
      await sourcesLoaded();

      fireEvent.click(screen.getByRole('button', { name: CANCEL_LABEL }));

      expect(done).toHaveBeenCalledTimes(1);
    });
  });

  describe('read-only', () => {
    it('disables every field and offers no action', async () => {
      const { container } = renderEditor(null, false);
      await sourcesLoaded();

      const form = within(container).getByRole('form');
      expect(within(form).getByLabelText(LIMIT_LABEL)).toBeDisabled();
      expect(within(form).getByLabelText(THRESHOLD_LABEL)).toBeDisabled();
      expect(within(form).getByRole('combobox', { name: SCOPE_LABEL })).toBeDisabled();
      expect(within(form).getByRole('checkbox', { name: HARD_LABEL })).toBeDisabled();
      expect(within(form).queryByRole('button')).not.toBeInTheDocument();
    });
  });
});

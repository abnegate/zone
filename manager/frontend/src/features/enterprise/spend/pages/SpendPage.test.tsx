import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { ApiError } from '../../../../api/ApiError';
import { clientModule } from '../../../../test/client';
import { chooseSelect } from '../../../../test/select';
import {
  ACTIVE_BUDGETS,
  ACTIVE_EMPTY_REPORT,
  ACTIVE_REPORT,
  EXPORT_FILENAME,
  LAPSED_BUDGETS,
  LAPSED_REPORT,
  MEMBERS,
  ORGANIZATION_ID,
  PLATFORM_WORKSPACE_ID,
  POPULATED_REPORT,
  UNLICENSED_BUDGETS,
  UNLICENSED_REPORT,
  WORKSPACES,
} from '../fixtures';
import {
  ADD_LABEL,
  AUTOMATION_LABEL,
  CANCEL_LABEL,
  CHART_CAPTION,
  DELETED_WORKSPACE,
  DOWNLOAD_LABEL,
  DOWNLOADED_LABEL,
  FROM_LABEL,
  HARD_LIMIT_NOTE,
  LAPSED_NOTICE,
  LIMIT_LABEL,
  LOADING_LABEL,
  NO_SPEND,
  NOTHING_COUNTED,
  PAGE_TITLE,
  RANGE_TOO_LONG,
  TO_LABEL,
  UNCOUNTED_FOOTNOTE,
  UPGRADE_NOTICE,
  WORKSPACE_FILTER_LABEL,
} from '../labels';
import { refusalMessage, uncountedLine } from '../messages';
import { monthToDate } from '../range';
import { SpendRefusal } from '../SpendRefusal';
import { SpendCode, type SpendReportResponse, UncountedReason } from '../schemas';

const report = mock();
const download = mock();
const budgets = mock();
const createBudget = mock();
const updateBudget = mock();
const deleteBudget = mock();
const saveDownload = mock();
const getWorkspaces = mock();
const getOrgMembers = mock();

mock.module('../api', () => ({
  spendApi: { report, download, budgets, createBudget, updateBudget, deleteBudget },
}));

mock.module('../../audit/save', () => ({ saveDownload }));

mock.module('../../../../api/client', () => clientModule({ getWorkspaces, getOrgMembers }));

let SpendPage: typeof import('./SpendPage').SpendPage;

beforeAll(async () => {
  ({ SpendPage } = await import('./SpendPage'));
});

afterAll(() => {
  mock.restore();
});

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <SpendPage organizationId={ORGANIZATION_ID} />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

async function loaded(response: SpendReportResponse, budgetsResponse = ACTIVE_BUDGETS) {
  report.mockResolvedValue(response);
  budgets.mockResolvedValue(budgetsResponse);
  const view = renderPage();
  await screen.findByRole('heading', { name: PAGE_TITLE });
  return view;
}

function region(name: string): HTMLElement {
  return screen.getByRole('region', { name });
}

describe('SpendPage', () => {
  beforeEach(() => {
    for (const fn of [
      report,
      download,
      budgets,
      createBudget,
      updateBudget,
      deleteBudget,
      saveDownload,
      getWorkspaces,
      getOrgMembers,
    ]) {
      fn.mockReset();
    }
    getWorkspaces.mockResolvedValue(WORKSPACES);
    getOrgMembers.mockResolvedValue({ members: MEMBERS });
  });

  it('shows a loading state until spend arrives', () => {
    report.mockImplementation(() => new Promise(() => {}));
    budgets.mockImplementation(() => new Promise(() => {}));
    renderPage();

    expect(screen.getByText(LOADING_LABEL)).toBeInTheDocument();
  });

  it('asks for month to date by default', async () => {
    await loaded(ACTIVE_REPORT);

    expect(report).toHaveBeenCalledWith(ORGANIZATION_ID, monthToDate());
  });

  describe('unlicensed', () => {
    it('shows the upgrade notice and nothing else', async () => {
      const { container } = await loaded(UNLICENSED_REPORT, UNLICENSED_BUDGETS);

      const notice = screen.getByRole('note');
      expect(notice).toHaveTextContent(UPGRADE_NOTICE);
      expect(within(notice).getByRole('link', { name: 'Go to License' })).toHaveAttribute(
        'href',
        '/settings/server/license'
      );
      expect(screen.queryByLabelText(FROM_LABEL)).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: DOWNLOAD_LABEL })).not.toBeInTheDocument();
      expect(screen.queryByRole('img', { name: CHART_CAPTION })).not.toBeInTheDocument();
      expect(screen.queryByRole('region', { name: 'Monthly budgets' })).not.toBeInTheDocument();
      expect(screen.queryByRole('region', { name: 'Not counted' })).not.toBeInTheDocument();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      expect(container).not.toHaveTextContent(/enterprise/i);
      expect(getWorkspaces).not.toHaveBeenCalled();
      expect(getOrgMembers).not.toHaveBeenCalled();
    });
  });

  describe('lapsed', () => {
    it('warns that reports stay readable and shows the budgets read-only', async () => {
      const { container } = await loaded(LAPSED_REPORT, LAPSED_BUDGETS);
      await screen.findByRole('region', { name: 'Monthly budgets' });

      const warning = screen.getByRole('status');
      expect(warning).toHaveClass('alert-warning');
      expect(warning).toHaveTextContent(LAPSED_NOTICE);
      expect(within(warning).getByRole('link')).toHaveAttribute('href', '/settings/server/license');
      expect(screen.queryByText(UPGRADE_NOTICE)).not.toBeInTheDocument();
      expect(screen.getByRole('img', { name: CHART_CAPTION })).toBeInTheDocument();

      const budgetsRegion = region('Monthly budgets');
      expect(within(budgetsRegion).getAllByRole('meter')).toHaveLength(3);
      expect(
        within(budgetsRegion).queryByRole('button', { name: /^Edit/ })
      ).not.toBeInTheDocument();
      expect(
        within(budgetsRegion).queryByRole('button', { name: /^Delete/ })
      ).not.toBeInTheDocument();
      expect(within(budgetsRegion).getByLabelText(LIMIT_LABEL)).toBeDisabled();
      expect(within(budgetsRegion).getByRole('checkbox')).toBeDisabled();
      expect(
        within(budgetsRegion).queryByRole('button', { name: ADD_LABEL })
      ).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeEnabled();
      expect(container).not.toHaveTextContent(/enterprise/i);
    });
  });

  describe('active', () => {
    it('draws one bar per day with an accessible table behind the chart', async () => {
      await loaded(ACTIVE_REPORT);

      const chart = screen.getByRole('img', { name: CHART_CAPTION });
      expect(within(chart).getAllByTestId('spend-bar')).toHaveLength(POPULATED_REPORT.daily.length);
      const table = screen.getByRole('table', { name: CHART_CAPTION });
      expect(within(table).getAllByRole('row')).toHaveLength(POPULATED_REPORT.daily.length + 1);
      expect(within(table).getByRole('rowheader', { name: 'Nov 1, 2026' })).toBeInTheDocument();
    });

    it('totals spend, requests and tokens in tiles', async () => {
      await loaded(ACTIVE_REPORT);

      const tiles = screen.getAllByTestId('analytics-tile');
      expect(tiles.map((tile) => tile.textContent)).toEqual([
        'Spend$64.23$64.226250',
        'Requests5,062',
        'Prompt tokens11,794,000',
        'Completion tokens3,037,500',
      ]);
      expect(screen.getByText('Counted through Nov 21 at 10:04 AM UTC')).toBeInTheDocument();
    });

    it('breaks spend down and labels deleted workspaces and automation', async () => {
      await loaded(ACTIVE_REPORT);

      const workspaces = screen.getByRole('table', { name: 'By workspace' });
      expect(
        within(workspaces)
          .getAllByRole('columnheader')
          .map((header) => header.textContent)
      ).toEqual(['Name', 'Spend', 'Requests', 'Tokens']);
      expect(within(workspaces).getByText('Platform')).toBeInTheDocument();
      expect(within(workspaces).getByText(DELETED_WORKSPACE)).toBeInTheDocument();
      const people = screen.getByRole('table', { name: 'By person' });
      expect(within(people).getByText(AUTOMATION_LABEL)).toBeInTheDocument();
      expect(within(people).getByText('Ada Lovelace')).toBeInTheDocument();
      expect(within(people).getByText('grace@example.com')).toBeInTheDocument();
      expect(within(screen.getByRole('table', { name: 'By model' })).getByText('qwen3:8b'));
      const providers = screen.getByRole('table', { name: 'By provider' });
      expect(within(providers).getByText('ollama')).toBeInTheDocument();
      expect(within(providers).getByText('$0.00')).toBeInTheDocument();
    });

    it('lists what is not counted with a fixed line each and the footnote', async () => {
      const { container } = await loaded(ACTIVE_REPORT);

      const uncounted = region('Not counted');
      const list = within(uncounted).getByRole('list');
      expect(
        within(list)
          .getAllByRole('listitem')
          .map((item) => item.textContent)
      ).toEqual([
        uncountedLine(UncountedReason.CodingAgent),
        uncountedLine(UncountedReason.DirectProvider),
      ]);
      expect(within(uncounted).getByText(UNCOUNTED_FOOTNOTE)).toBeInTheDocument();
      expect(within(uncounted).getByRole('link')).toHaveAttribute(
        'href',
        '/settings/organization/ai'
      );
      expect(screen.getByText(HARD_LIMIT_NOTE)).toBeInTheDocument();
      expect(container).not.toHaveTextContent(/enterprise/i);
    });

    it('shows an empty month without inventing numbers', async () => {
      await loaded(ACTIVE_EMPTY_REPORT);

      expect(screen.getByText(NO_SPEND)).toBeInTheDocument();
      expect(screen.getByText(NOTHING_COUNTED)).toBeInTheDocument();
      expect(screen.queryByRole('img', { name: CHART_CAPTION })).not.toBeInTheDocument();
      expect(within(region('Not counted')).queryByRole('list')).not.toBeInTheDocument();
      expect(within(region('Not counted')).getByText(UNCOUNTED_FOOTNOTE)).toBeInTheDocument();
    });

    it('reloads the report for a workspace filter from the workspaces source', async () => {
      await loaded(ACTIVE_REPORT);
      await waitFor(() => expect(getWorkspaces).toHaveBeenCalledWith(ORGANIZATION_ID));
      await waitFor(() =>
        expect(screen.getByRole('combobox', { name: WORKSPACE_FILTER_LABEL })).not.toBeDisabled()
      );

      chooseSelect(WORKSPACE_FILTER_LABEL, 'Platform');

      await waitFor(() =>
        expect(report).toHaveBeenLastCalledWith(ORGANIZATION_ID, {
          ...monthToDate(),
          workspace_id: PLATFORM_WORKSPACE_ID,
        })
      );
    });

    it('holds the report and explains a start after the end', async () => {
      await loaded(ACTIVE_REPORT);
      const calls = report.mock.calls.length;

      fireEvent.change(screen.getByLabelText(FROM_LABEL), { target: { value: '2026-12-31' } });
      fireEvent.change(screen.getByLabelText(TO_LABEL), { target: { value: '2026-12-01' } });

      expect(screen.getByText(refusalMessage(SpendCode.RangeInvalid))).toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
      expect(report).toHaveBeenCalledTimes(calls);
    });

    it('holds the report and explains a range longer than the server reads', async () => {
      await loaded(ACTIVE_REPORT);
      const calls = report.mock.calls.length;

      fireEvent.change(screen.getByLabelText(FROM_LABEL), { target: { value: '2025-01-01' } });
      fireEvent.change(screen.getByLabelText(TO_LABEL), { target: { value: '2026-01-02' } });

      expect(screen.getByText(RANGE_TOO_LONG)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: DOWNLOAD_LABEL })).toBeDisabled();
      expect(report).toHaveBeenCalledTimes(calls);
    });

    it('downloads the CSV through saveDownload and names the file', async () => {
      const file = { blob: new Blob(['date\r\n']), filename: EXPORT_FILENAME };
      download.mockResolvedValue(file);
      await loaded(ACTIVE_REPORT);

      fireEvent.click(screen.getByRole('button', { name: DOWNLOAD_LABEL }));

      await waitFor(() => expect(saveDownload).toHaveBeenCalledTimes(1));
      expect(saveDownload.mock.calls[0][0]).toEqual(file);
      expect(download).toHaveBeenCalledWith(ORGANIZATION_ID, monthToDate());
      expect(await screen.findByText(`${DOWNLOADED_LABEL} ${EXPORT_FILENAME}`)).toBeInTheDocument();
    });

    it('explains a refused download', async () => {
      const { SpendRefusal } = await import('../SpendRefusal');
      download.mockRejectedValue(
        new SpendRefusal({ error: 'raw', code: SpendCode.LicenseRequired }, 403)
      );
      await loaded(ACTIVE_REPORT);

      fireEvent.click(screen.getByRole('button', { name: DOWNLOAD_LABEL }));

      expect(await screen.findByRole('alert')).toHaveTextContent(
        refusalMessage(SpendCode.LicenseRequired)
      );
      expect(saveDownload).not.toHaveBeenCalled();
    });
  });

  describe('failures', () => {
    it('offers a retry when the report fails to load', async () => {
      report
        .mockRejectedValueOnce(new ApiError('Failed to load spend: Organization not found', 404))
        .mockResolvedValueOnce(ACTIVE_REPORT);
      budgets.mockResolvedValue(ACTIVE_BUDGETS);
      renderPage();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load spend: Organization not found');
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('img', { name: CHART_CAPTION });
      expect(report).toHaveBeenCalledTimes(2);
    });

    it('offers a retry when the budgets fail to load and keeps the report', async () => {
      report.mockResolvedValue(ACTIVE_REPORT);
      budgets
        .mockRejectedValueOnce(new ApiError('Failed to load budgets: Forbidden', 403))
        .mockResolvedValueOnce(ACTIVE_BUDGETS);
      renderPage();

      const alert = await screen.findByRole('alert');
      expect(alert).toHaveTextContent('Failed to load budgets: Forbidden');
      expect(screen.getByRole('img', { name: CHART_CAPTION })).toBeInTheDocument();
      fireEvent.click(within(alert).getByRole('button', { name: 'Retry' }));

      await screen.findByRole('region', { name: 'Monthly budgets' });
      expect(budgets).toHaveBeenCalledTimes(2);
    });
  });

  describe('budgets', () => {
    it('shows each budget with its state and how much is used', async () => {
      await loaded(ACTIVE_REPORT);
      const budgetsRegion = await screen.findByRole('region', { name: 'Monthly budgets' });

      const meters = within(budgetsRegion).getAllByRole('meter');
      expect(meters.map((meter) => meter.getAttribute('aria-valuetext'))).toEqual([
        '$64.23 of $250.00',
        '$39.82 of $35.00',
        '$36.50 of $40.00',
      ]);
      expect(meters.map((meter) => meter.getAttribute('aria-valuenow'))).toEqual([
        '25',
        '100',
        '91',
      ]);
      expect(within(budgetsRegion).getByText('Used up')).toBeInTheDocument();
      expect(within(budgetsRegion).getByText('Past its warning')).toBeInTheDocument();
      expect(within(budgetsRegion).getByText('On track')).toBeInTheDocument();
    });

    it('deletes a budget after confirming', async () => {
      deleteBudget.mockResolvedValue(undefined);
      await loaded(ACTIVE_REPORT);
      const budgetsRegion = await screen.findByRole('region', { name: 'Monthly budgets' });

      fireEvent.click(
        within(budgetsRegion).getByRole('button', { name: 'Delete the budget for Platform' })
      );
      const dialog = await screen.findByRole('dialog');
      expect(dialog).toHaveTextContent('Delete the budget for Platform?');
      fireEvent.click(within(dialog).getByRole('button', { name: 'Delete budget' }));

      await waitFor(() =>
        expect(deleteBudget).toHaveBeenCalledWith(ORGANIZATION_ID, ACTIVE_BUDGETS.budgets[1].id)
      );
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
      await waitFor(() => expect(budgets).toHaveBeenCalledTimes(2));
    });

    it('edits only what changed on an existing budget', async () => {
      updateBudget.mockResolvedValue(ACTIVE_BUDGETS.budgets[2]);
      await loaded(ACTIVE_REPORT);
      const budgetsRegion = await screen.findByRole('region', { name: 'Monthly budgets' });

      fireEvent.click(
        within(budgetsRegion).getByRole('button', { name: 'Edit the budget for Ada Lovelace' })
      );
      const editor = within(budgetsRegion).getByRole('form', { name: 'Edit budget' });
      expect(within(editor).getByLabelText(LIMIT_LABEL)).toHaveValue('40');
      fireEvent.change(within(editor).getByLabelText(LIMIT_LABEL), { target: { value: '60' } });
      fireEvent.click(within(editor).getByRole('button', { name: 'Save budget' }));

      await waitFor(() =>
        expect(updateBudget).toHaveBeenCalledWith(ORGANIZATION_ID, ACTIVE_BUDGETS.budgets[2].id, {
          limit_microdollars: 60_000_000,
        })
      );
      await waitFor(() =>
        expect(
          within(budgetsRegion).getByRole('form', { name: 'Add a budget' })
        ).toBeInTheDocument()
      );
    });

    it('drops a refused add once another budget is edited, so it does not reappear', async () => {
      createBudget.mockRejectedValue(
        new SpendRefusal({ error: 'raw', code: SpendCode.BudgetExists }, 409)
      );
      await loaded(ACTIVE_REPORT);
      const budgetsRegion = await screen.findByRole('region', { name: 'Monthly budgets' });
      const adder = within(budgetsRegion).getByRole('form', { name: 'Add a budget' });
      fireEvent.change(within(adder).getByLabelText(LIMIT_LABEL), { target: { value: '50' } });
      fireEvent.click(within(adder).getByRole('button', { name: ADD_LABEL }));
      const refused = refusalMessage(SpendCode.BudgetExists);
      await within(budgetsRegion).findByText(refused);

      fireEvent.click(
        within(budgetsRegion).getByRole('button', { name: 'Edit the budget for Ada Lovelace' })
      );
      const editor = within(budgetsRegion).getByRole('form', { name: 'Edit budget' });
      fireEvent.click(within(editor).getByRole('button', { name: CANCEL_LABEL }));

      expect(within(budgetsRegion).getByRole('form', { name: 'Add a budget' })).toBeInTheDocument();
      expect(within(budgetsRegion).queryByText(refused)).not.toBeInTheDocument();
    });
  });
});

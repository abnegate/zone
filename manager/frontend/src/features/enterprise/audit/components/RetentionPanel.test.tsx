import { afterAll, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AuditRefusal } from '../AuditRefusal';
import {
  CAPPED_RETENTION,
  COMMUNITY_RETENTION,
  COMMUNITY_SAVED_RETENTION,
  ENTERPRISE_RETENTION,
  FOREVER_RETENTION,
  LAPSED_RETENTION,
  ORGANIZATION_SCOPE,
  WAITING_RETENTION,
} from '../fixtures';
import {
  CUTOFF_LABEL,
  KEEP_FOREVER_LABEL,
  KEPT_FOREVER,
  NOT_SWEPT,
  READ_ONLY_LAPSED,
  READ_ONLY_UNLICENSED,
  RETENTION_DAYS_LABEL,
  RETENTION_NOT_APPLIED,
  RETENTION_SAVED,
  SAVE_RETENTION_LABEL,
} from '../labels';
import {
  deletedText,
  refusalMessage,
  retentionSummary,
  retentionText,
  waitMessage,
} from '../messages';
import { AuditCode, RetentionWait } from '../schemas';
import type { RetentionResponse } from '../types';

const setRetention = mock();

mock.module('../api', () => ({
  auditApi: {
    destinations: mock(),
    destination: mock(),
    createDestination: mock(),
    updateDestination: mock(),
    deleteDestination: mock(),
    testDestination: mock(),
    retention: mock(),
    setRetention,
    download: mock(),
  },
}));

let RetentionPanel: typeof import('./RetentionPanel').RetentionPanel;

beforeAll(async () => {
  ({ RetentionPanel } = await import('./RetentionPanel'));
});

afterAll(() => {
  mock.restore();
});

function renderPanel(retention: RetentionResponse) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <RetentionPanel scope={ORGANIZATION_SCOPE} retention={retention} />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

function days(): HTMLElement {
  return screen.getByLabelText(RETENTION_DAYS_LABEL);
}

function save(): HTMLElement {
  return screen.getByRole('button', { name: SAVE_RETENTION_LABEL });
}

describe('RetentionPanel', () => {
  beforeEach(() => {
    setRetention.mockReset();
  });

  it('shows the period with its cutoff, counts and last sweep', () => {
    renderPanel(ENTERPRISE_RETENTION);

    expect(screen.getByText(retentionText(90))).toBeInTheDocument();
    expect(screen.getByText(CUTOFF_LABEL)).toBeInTheDocument();
    expect(screen.getByText(/1,204 events deleted/)).toBeInTheDocument();
    expect(days()).toHaveValue('90');
    expect(screen.getByRole('checkbox', { name: KEEP_FOREVER_LABEL })).not.toBeChecked();
    expect(save()).toBeDisabled();
  });

  it('says when no sweep has run yet', () => {
    renderPanel({ ...ENTERPRISE_RETENTION, swept_at: null });

    expect(screen.getByText(NOT_SWEPT)).toBeInTheDocument();
    expect(screen.queryByText(deletedText(ENTERPRISE_RETENTION.deleted_count))).toBeNull();
  });

  it('explains which destinations retention is waiting on', () => {
    renderPanel(WAITING_RETENTION);

    expect(screen.getByText('Splunk HEC')).toBeInTheDocument();
    expect(screen.getByText(waitMessage(RetentionWait.Failing))).toBeInTheDocument();
  });

  it('marks a count the server stopped at as a lower bound', () => {
    renderPanel(CAPPED_RETENTION);

    expect(screen.getAllByText('10,000+')).toHaveLength(2);
  });

  it('sets a new period', async () => {
    setRetention.mockResolvedValue({ ...ENTERPRISE_RETENTION, days: 30 });
    renderPanel(ENTERPRISE_RETENTION);

    fireEvent.change(days(), { target: { value: '30' } });
    fireEvent.click(save());

    expect(await screen.findByText(RETENTION_SAVED)).toBeInTheDocument();
    expect(setRetention).toHaveBeenCalledWith(ORGANIZATION_SCOPE, { days: 30 });
  });

  it('keeps events forever', async () => {
    setRetention.mockResolvedValue(FOREVER_RETENTION);
    renderPanel(ENTERPRISE_RETENTION);

    fireEvent.click(screen.getByRole('checkbox', { name: KEEP_FOREVER_LABEL }));
    expect(days()).toBeDisabled();
    fireEvent.click(save());

    await waitFor(() =>
      expect(setRetention).toHaveBeenCalledWith(ORGANIZATION_SCOPE, { days: null })
    );
  });

  it('starts from keep forever and refuses a period that is not a whole number of days', () => {
    renderPanel(FOREVER_RETENTION);

    expect(screen.getByText(KEPT_FOREVER)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: KEEP_FOREVER_LABEL }));
    fireEvent.change(days(), { target: { value: '1.5' } });
    fireEvent.click(save());

    expect(screen.getByText(refusalMessage(AuditCode.RetentionInvalid))).toBeInTheDocument();
    expect(setRetention).not.toHaveBeenCalled();
  });

  it('shows a refused change', async () => {
    setRetention.mockRejectedValue(
      new AuditRefusal({ error: 'Bad', code: AuditCode.RetentionInvalid }, 400)
    );
    renderPanel(ENTERPRISE_RETENTION);

    fireEvent.change(days(), { target: { value: '7' } });
    fireEvent.click(save());

    expect(await screen.findByRole('alert')).toHaveTextContent(
      refusalMessage(AuditCode.RetentionInvalid)
    );
  });

  it('shows Community that retention needs a license and nothing to change', () => {
    renderPanel(COMMUNITY_RETENTION);

    expect(screen.getAllByText(/kept forever/)).toHaveLength(1);
    expect(screen.getByText(KEPT_FOREVER)).toBeInTheDocument();
    expect(screen.getByText(RETENTION_NOT_APPLIED)).toBeInTheDocument();
    expect(screen.getByText(READ_ONLY_UNLICENSED)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: SAVE_RETENTION_LABEL })).toBeNull();
  });

  it('says a period saved before the license was removed resumes once one is installed', () => {
    renderPanel(COMMUNITY_SAVED_RETENTION);

    expect(screen.getByText(retentionSummary(90, false))).toBeInTheDocument();
    expect(screen.queryByText(retentionText(90))).toBeNull();
    expect(screen.queryByText(RETENTION_NOT_APPLIED)).toBeNull();
    expect(screen.queryByText(CUTOFF_LABEL)).toBeNull();
    expect(screen.getByText(READ_ONLY_UNLICENSED)).toBeInTheDocument();
  });

  it('keeps applying a lapsed period read-only', () => {
    renderPanel(LAPSED_RETENTION);

    expect(screen.getByText(retentionText(90))).toBeInTheDocument();
    expect(screen.queryByText(RETENTION_NOT_APPLIED)).toBeNull();
    expect(screen.getAllByText(waitMessage(RetentionWait.Paused))).toHaveLength(2);
    expect(screen.getByText(READ_ONLY_LAPSED)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: SAVE_RETENTION_LABEL })).toBeNull();
  });
});

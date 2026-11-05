import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ChatSubagents } from '../hooks/useChatSubagents';
import type { ChatSubagent, SubagentRequest } from '../subagentTypes';
import { SubagentsBox } from './SubagentsBox';

const REVIEWER: ChatSubagent = {
  id: 'subagent-1',
  chat_id: 'chat-1',
  message_id: 'message-1',
  external_id: 'toolu_01Agent',
  description: 'Review the cart change',
  kind: 'general-purpose',
  model: 'sonnet',
  background: false,
  status: 'completed',
  tool_uses: 3,
  duration_ms: 185_000,
  tokens: 18_000,
  last_tool: 'Edit',
  summary: 'The rounding is fixed and covered by a test.',
  error: null,
  steps: [
    { id: 'toolu_x1', name: 'Read', subject: 'src/cart.ts', status: 'succeeded' },
    { id: 'toolu_x2', name: 'Bash', subject: 'bun test cart', status: 'failed' },
    { id: 'toolu_x3', name: 'Grep', subject: null, status: 'running' },
  ],
  started_at: '2026-11-05T10:00:00Z',
  finished_at: '2026-11-05T10:03:05Z',
  updated_at: '2026-11-05T10:03:05Z',
};

const CODEX: ChatSubagent = {
  ...REVIEWER,
  id: 'subagent-2',
  external_id: 'call_codex',
  description: 'Explore the checkout flow',
  kind: null,
  model: null,
  tool_uses: null,
  duration_ms: null,
  last_tool: null,
  summary: null,
  steps: [],
  started_at: '2026-11-05T10:01:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:01:00Z',
};

const FAILED: ChatSubagent = {
  ...REVIEWER,
  id: 'subagent-3',
  description: '',
  status: 'failed',
  summary: 'Never shown beside an error',
  error: 'The agent ran out of turns',
  steps: [],
};

const COLLAPSED_KEY = 'zone:chat-panel:user-1:subagents';

function listed(subagents: ChatSubagent[], error: string | null = null): ChatSubagents {
  return { subagents, error, refresh: mock(async () => undefined) };
}

function box(subagents: ChatSubagents, shown: SubagentRequest | null = null, onShown = mock()) {
  return <SubagentsBox userId="user-1" subagents={subagents} shown={shown} onShown={onShown} />;
}

function row(id: string): HTMLElement {
  const found = document.querySelector<HTMLElement>(`[data-subagent="${id}"]`);
  if (!found) throw new Error(`no row for ${id}`);
  return found;
}

describe('SubagentsBox', () => {
  const scrolled = mock();
  const original = Element.prototype.scrollIntoView;

  beforeEach(() => {
    localStorage.clear();
    scrolled.mockReset();
    Element.prototype.scrollIntoView = scrolled;
  });

  afterEach(() => {
    Element.prototype.scrollIntoView = original;
  });

  it('lists the agents under an Agents heading with their count', () => {
    render(box(listed([REVIEWER, CODEX])));

    expect(screen.getByRole('button', { name: 'Agents, 2' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    expect(screen.getAllByRole('listitem').filter((item) => item.dataset.subagent)).toHaveLength(2);
  });

  it('shows what an agent was, how it went, and what it found', () => {
    render(box(listed([REVIEWER])));

    const item = row(REVIEWER.id);
    expect(within(item).getByText('Completed')).toHaveClass('ui-badge-success');
    expect(within(item).getByText('general-purpose')).toBeInTheDocument();
    expect(within(item).getByText('sonnet')).toBeInTheDocument();
    expect(within(item).getByText('3 tools')).toBeInTheDocument();
    expect(within(item).getByText('3m 05s')).toBeInTheDocument();
    expect(within(item).getByText('Edit')).toBeInTheDocument();
    expect(within(item).getByText(REVIEWER.summary ?? '')).toHaveClass('chat-subagent-summary');
  });

  it('folds the steps away until the agent is expanded, each with its status', () => {
    render(box(listed([REVIEWER])));

    const toggle = screen.getByRole('button', { name: REVIEWER.description });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    const steps = document.getElementById(toggle.getAttribute('aria-controls') ?? '');
    expect(steps).toHaveAttribute('hidden');

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute('aria-expanded', 'true');
    expect(steps).not.toHaveAttribute('hidden');
    const rows = within(steps as HTMLElement).getAllByRole('listitem');
    expect(rows.map((step) => step.textContent)).toEqual([
      'SucceededReadsrc/cart.ts',
      'FailedBashbun test cart',
      'RunningGrep',
    ]);
    expect(within(rows[0]).getByText('src/cart.ts').tagName).toBe('CODE');
    expect(rows[1]).toHaveClass('chat-subagent-step--failed');
  });

  it('gives an agent that reported no steps, like a Codex one, nothing to expand', () => {
    render(box(listed([CODEX])));

    const item = row(CODEX.id);
    expect(within(item).queryByRole('button')).toBeNull();
    expect(within(item).getByText(CODEX.description)).toBeInTheDocument();
    expect(item.querySelector('.chat-subagent-meta')).toBeNull();
    expect(item.querySelector('.chat-subagent-summary')).toBeNull();
  });

  it('shows a failed agent by its error rather than its summary, and names one with no description', () => {
    render(box(listed([FAILED])));

    const item = row(FAILED.id);
    expect(within(item).getByText('Agent')).toBeInTheDocument();
    expect(within(item).getByText('Failed')).toHaveClass('ui-badge-destructive');
    expect(within(item).getByText(FAILED.error ?? '')).toHaveClass('chat-subagent-error');
    expect(within(item).queryByText(FAILED.summary ?? '')).toBeNull();
  });

  it('marks a running agent as live and counts its time from its start', () => {
    const started = new Date(Date.now() - 42_000).toISOString();
    render(box(listed([{ ...CODEX, status: 'running', started_at: started }])));

    const item = row(CODEX.id);
    const status = within(item).getByText('Running');
    expect(status).toHaveClass('ui-badge-info');
    expect(status.querySelector('.chat-subagent-pulse')).not.toBeNull();
    expect(item.querySelector('.chat-subagent-duration')?.textContent).toMatch(/^4[2-3]s$/);
  });

  it('opens itself and the agent a chip asks for, scrolls to it and focuses it', async () => {
    localStorage.setItem(COLLAPSED_KEY, 'true');
    const onShown = mock();
    const { rerender } = render(box(listed([REVIEWER, CODEX]), null, onShown));
    expect(screen.getByRole('button', { name: 'Agents, 2' })).toHaveAttribute(
      'aria-expanded',
      'false'
    );

    rerender(box(listed([REVIEWER, CODEX]), { id: REVIEWER.id, nonce: 1 }, onShown));

    expect(onShown).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Agents, 2' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    const toggle = screen.getByRole('button', { name: REVIEWER.description });
    await waitFor(() => expect(toggle).toHaveAttribute('aria-expanded', 'true'));
    await waitFor(() => expect(document.activeElement).toBe(toggle));
    expect(scrolled).toHaveBeenCalledWith({ block: 'nearest' });
  });

  it('expands the same agent again when asked again after it was folded', async () => {
    const { rerender } = render(box(listed([REVIEWER]), { id: REVIEWER.id, nonce: 1 }));
    const toggle = screen.getByRole('button', { name: REVIEWER.description });
    await waitFor(() => expect(toggle).toHaveAttribute('aria-expanded', 'true'));

    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    rerender(box(listed([REVIEWER]), { id: REVIEWER.id, nonce: 2 }));

    await waitFor(() => expect(toggle).toHaveAttribute('aria-expanded', 'true'));
  });

  it('collapses, and remembers that it was collapsed', () => {
    const { unmount } = render(box(listed([REVIEWER])));

    fireEvent.click(screen.getByRole('button', { name: 'Agents, 1' }));

    expect(localStorage.getItem(COLLAPSED_KEY)).toBe('true');
    unmount();
    render(box(listed([REVIEWER])));
    expect(screen.getByRole('button', { name: 'Agents, 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    );
  });

  it('says when the agents could not be read', () => {
    render(box(listed([REVIEWER], 'Failed to load agents')));

    expect(screen.getByRole('alert')).toHaveTextContent('Failed to load agents');
  });
});

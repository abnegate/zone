import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ChatSubagent } from '../subagentTypes';
import { SubagentChip } from './SubagentChip';
import { SubagentChips } from './SubagentChips';

const RUNNING: ChatSubagent = {
  id: 'subagent-1',
  chat_id: 'chat-1',
  message_id: 'message-1',
  external_id: 'toolu_01Agent',
  description: 'Review the cart change',
  kind: 'general-purpose',
  model: 'sonnet',
  background: true,
  status: 'running',
  tool_uses: 2,
  duration_ms: null,
  tokens: null,
  last_tool: 'Read',
  summary: null,
  error: null,
  step_count: 0,
  revision: 1,
  started_at: '2026-11-05T10:00:00Z',
  finished_at: null,
  updated_at: '2026-11-05T10:00:00Z',
};

const DONE: ChatSubagent = {
  ...RUNNING,
  id: 'subagent-2',
  description: 'Run the tests',
  status: 'completed',
};

describe('SubagentChip', () => {
  it('names the agent and its status, with decoration hidden from assistive technology', () => {
    render(<SubagentChip subagent={RUNNING} onShow={mock()} />);

    const chip = screen.getByRole('button', {
      name: 'Agent: Review the cart change, Running. Show in the agents box',
    });
    expect(chip).toHaveTextContent('Agent: Review the cart change');
    expect(within(chip).getByText('Running')).toHaveClass('ui-badge-info');
    expect(chip.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
    expect(chip.querySelector('.chat-subagent-pulse')).toHaveAttribute('aria-hidden', 'true');
  });

  it('is named by the text it shows, so a spoken command can find it', () => {
    render(<SubagentChip subagent={RUNNING} onShow={mock()} />);

    const chip = screen.getByRole('button');
    const visible = chip.textContent ?? '';
    expect(visible).toBe('Agent: Review the cart changeRunning');
    expect(chip.getAttribute('aria-label')).toStartWith('Agent: Review the cart change, Running');
  });

  it('stops pulsing once the agent is done', () => {
    render(<SubagentChip subagent={DONE} onShow={mock()} />);

    const chip = screen.getByRole('button', {
      name: 'Agent: Run the tests, Completed. Show in the agents box',
    });
    expect(within(chip).getByText('Completed')).toHaveClass('ui-badge-success');
    expect(chip.querySelector('.chat-subagent-pulse')).toBeNull();
  });

  it('names an agent with no description plainly', () => {
    render(<SubagentChip subagent={{ ...DONE, description: '' }} onShow={mock()} />);

    const chip = screen.getByRole('button', { name: 'Agent, Completed. Show in the agents box' });
    expect(within(chip).getByText('Agent')).toHaveClass('chat-subagent-chip-name');
    expect(chip).not.toHaveTextContent('Agent: Agent');
  });

  it('asks to show the agent it names when chosen', () => {
    const onShow = mock();
    render(<SubagentChip subagent={RUNNING} onShow={onShow} />);

    fireEvent.click(screen.getByRole('button'));

    expect(onShow).toHaveBeenCalledWith(RUNNING.id);
  });
});

describe('SubagentChips', () => {
  it('lists a chip for each agent the message started', () => {
    const onShow = mock();
    render(<SubagentChips subagents={[RUNNING, DONE]} onShow={onShow} />);

    const chips = within(screen.getByRole('list', { name: 'Agents' })).getAllByRole('button');
    expect(chips.map((chip) => chip.getAttribute('aria-label'))).toEqual([
      'Agent: Review the cart change, Running. Show in the agents box',
      'Agent: Run the tests, Completed. Show in the agents box',
    ]);
    fireEvent.click(chips[1]);
    expect(onShow).toHaveBeenCalledWith(DONE.id);
  });

  it('takes no room when the message started no agents', () => {
    const { container } = render(<SubagentChips subagents={[]} onShow={mock()} />);

    expect(container.innerHTML).toBe('');
  });
});

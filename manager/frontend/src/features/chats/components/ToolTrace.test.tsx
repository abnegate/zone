import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ToolCallRecord } from '../types';
import { ToolTrace } from './ToolTrace';

const call = (overrides: Partial<ToolCallRecord> = {}): ToolCallRecord => ({
  id: 'call_1',
  name: 'search_knowledge',
  arguments: '{"query":"deploys"}',
  success: true,
  detail: '3 passages',
  duration_ms: 128,
  ...overrides,
});

describe('ToolTrace', () => {
  it('renders nothing when there are no tool calls', () => {
    const { container } = render(<ToolTrace calls={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it('shows the thinking that preceded a tool call', () => {
    render(<ToolTrace calls={[call({ reasoning: 'I should search workspace docs first.' })]} />);

    expect(screen.getByText('I should search workspace docs first.')).toBeInTheDocument();
    expect(screen.getByTestId('reasoning')).toHaveAttribute('open');
  });

  it('describes the tool in plain language rather than by its wire name', () => {
    render(<ToolTrace calls={[call()]} />);

    expect(screen.getByText('Searched the knowledge base')).toBeInTheDocument();
    expect(screen.queryByText('search_knowledge')).not.toBeInTheDocument();
    expect(screen.getByText('3 passages')).toBeInTheDocument();
  });

  it('falls back to the raw name for a tool it does not know', () => {
    render(<ToolTrace calls={[call({ name: 'some_new_tool' })]} />);

    expect(screen.getByText('some_new_tool')).toBeInTheDocument();
  });

  it('marks failed and running calls distinctly', () => {
    render(
      <ToolTrace
        calls={[
          call({ id: 'a', success: false, detail: 'Error: search failed' }),
          call({ id: 'b', pending: true, detail: 'Running…', duration_ms: 0 }),
        ]}
      />
    );

    const [failed, pending] = screen.getAllByTestId('tool-call');
    expect(failed.closest('li')).toHaveClass('tool-call--failed');
    expect(pending.closest('li')).toHaveClass('tool-call--pending');
  });

  it('hides the duration until the call finishes', () => {
    const { rerender } = render(<ToolTrace calls={[call({ pending: true, duration_ms: 0 })]} />);
    expect(screen.queryByText('128ms')).not.toBeInTheDocument();

    rerender(<ToolTrace calls={[call()]} />);
    expect(screen.getByText('128ms')).toBeInTheDocument();
  });

  it('formats durations over a second in seconds', () => {
    render(<ToolTrace calls={[call({ duration_ms: 1540 })]} />);

    expect(screen.getByText('1.5s')).toBeInTheDocument();
  });

  it('reveals the arguments the model passed when expanded', () => {
    render(<ToolTrace calls={[call()]} />);

    expect(screen.queryByText(/deploys/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tool-call'));

    // Pretty-printed so a long argument object stays readable.
    expect(screen.getByText(/"query": "deploys"/)).toBeInTheDocument();
  });

  it('cannot be expanded when the call took no arguments', () => {
    render(<ToolTrace calls={[call({ name: 'list_tasks', arguments: '{}' })]} />);

    expect(screen.getByTestId('tool-call')).toBeDisabled();
  });

  it('shows unparseable arguments verbatim rather than dropping them', () => {
    render(<ToolTrace calls={[call({ arguments: '{"query": ' })]} />);

    fireEvent.click(screen.getByTestId('tool-call'));

    expect(screen.getByText('{"query":')).toBeInTheDocument();
  });

  it('asks the reader to approve a mutating tool', () => {
    const decisions: Array<[string, boolean]> = [];
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
          }),
        ]}
        onDecide={(id, approved) => {
          decisions.push([id, approved]);
        }}
      />
    );

    expect(screen.getByText('Waiting for approval…')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('tool-approve'));
    expect(decisions).toEqual([['call_1', true]]);
  });

  it('lets the reader deny a mutating tool', () => {
    const decisions: Array<[string, boolean]> = [];
    render(
      <ToolTrace
        calls={[
          call({
            name: 'run_shell',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
          }),
        ]}
        onDecide={(id, approved) => {
          decisions.push([id, approved]);
        }}
      />
    );

    fireEvent.click(screen.getByTestId('tool-deny'));
    expect(decisions).toEqual([['call_1', false]]);
  });

  it('does not show approval buttons without a decision handler', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'pending',
          }),
        ]}
      />
    );

    expect(screen.queryByTestId('tool-approve')).not.toBeInTheDocument();
    expect(screen.queryByTestId('tool-deny')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-call').closest('li')).toHaveClass('tool-call--approval');
  });

  it('shows the reason the model gave for a call still awaiting approval', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'run_shell',
            arguments: '{"command":"rm -rf build"}',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
            reason: 'The user asked me to clear the stale build output.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(
      screen.getByText('The user asked me to clear the stale build output.')
    ).toBeInTheDocument();
    expect(screen.getByTestId('tool-approve')).toBeInTheDocument();
  });

  it('keeps the reason on the row after the call has completed', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'create_pull_request',
            detail: 'Opened #42',
            reason: 'The user asked me to open the pull request.',
          }),
        ]}
      />
    );

    expect(screen.getByText('Opened #42')).toBeInTheDocument();
    expect(screen.getByText('The user asked me to open the pull request.')).toBeInTheDocument();
  });

  it('labels the reason as stated by the model, not observed by the server', () => {
    render(<ToolTrace calls={[call({ name: 'write_file', reason: 'Persist the config.' })]} />);

    expect(screen.getByTestId('tool-call-reason')).toHaveTextContent('Reason, stated by the model');
  });

  it('says so out loud when a side-effecting call gave no reason', () => {
    render(<ToolTrace calls={[call({ name: 'send_message', detail: 'Message sent' })]} />);

    expect(screen.getByTestId('tool-call-reason')).toBeInTheDocument();
    expect(screen.getByText('No reason given')).toBeInTheDocument();
  });

  it('treats a blank reason as no reason rather than an empty line', () => {
    render(<ToolTrace calls={[call({ name: 'apply_patch', reason: '   ' })]} />);

    expect(screen.getByTestId('tool-call-reason')).toHaveTextContent('No reason given');
  });

  it('leaves read-only tools out of it entirely', () => {
    render(<ToolTrace calls={[call({ name: 'search_knowledge' })]} />);

    expect(screen.queryByTestId('tool-call-reason')).not.toBeInTheDocument();
  });

  it('shows a reason volunteered by a tool this client has never heard of', () => {
    render(
      <ToolTrace
        calls={[call({ name: 'a_tool_from_a_later_release', reason: 'Because I must.' })]}
      />
    );

    expect(screen.getByText('Because I must.')).toBeInTheDocument();
  });

  it('hides approval buttons after the reader has decided', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'approved',
            detail: 'Approved. Running…',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.queryByTestId('tool-approve')).not.toBeInTheDocument();
  });
});

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
});

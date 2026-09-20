import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ToolCallRecord } from '../types';
import { Activity } from './Activity';

const call = (overrides: Partial<ToolCallRecord> = {}): ToolCallRecord => ({
  id: 'call_1',
  name: 'search_knowledge',
  arguments: '{"query":"deploys"}',
  success: true,
  detail: '3 passages',
  duration_ms: 128,
  ...overrides,
});

const search = call({ reasoning: 'Search the workspace first.' });
const read = call({
  id: 'call_2',
  name: 'read_document',
  arguments: '{"id":"doc-1"}',
  detail: '1 document',
  duration_ms: 40,
  reasoning: 'That hit looks right; read it.',
});

const follows = (first: HTMLElement, second: HTMLElement): boolean =>
  Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

describe('Activity', () => {
  it('renders nothing for a turn that neither thought nor called a tool', () => {
    const { container } = render(
      <Activity reasoning="  " calls={[]} live={false} answered={false} />
    );
    expect(container.firstChild).toBeNull();
  });

  it('labels the thinking once per turn and keeps each segment beside the call it preceded', () => {
    render(
      <Activity
        reasoning="Fridays are the deploy window."
        calls={[search, read]}
        live={false}
        answered={false}
      />
    );

    expect(screen.getAllByText('Reasoning')).toHaveLength(1);
    expect(screen.getAllByTestId('reasoning')).toHaveLength(3);

    const label = screen.getByRole('button', { name: 'Reasoning' });
    const firstThought = screen.getByText('Search the workspace first.');
    const firstTool = screen.getByText('Searched the knowledge base');
    const secondThought = screen.getByText('That hit looks right; read it.');
    const secondTool = screen.getByText('Read a workspace document');
    const leftover = screen.getByText('Fridays are the deploy window.');

    expect(follows(label, firstThought)).toBe(true);
    expect(follows(firstThought, firstTool)).toBe(true);
    expect(follows(firstTool, secondThought)).toBe(true);
    expect(follows(secondThought, secondTool)).toBe(true);
    expect(follows(secondTool, leftover)).toBe(true);
  });

  it('folds the thinking of a finished turn behind its label and leaves the tool rows in view', () => {
    render(
      <Activity
        reasoning="Fridays are the deploy window."
        calls={[search, read]}
        live={false}
        answered={false}
      />
    );

    const label = screen.getByRole('button', { name: 'Reasoning' });
    expect(label).toHaveAttribute('aria-expanded', 'false');
    for (const segment of screen.getAllByTestId('reasoning')) {
      expect(segment).toHaveAttribute('hidden');
    }
    for (const row of screen.getAllByTestId('tool-call')) {
      expect(row.closest('[hidden]')).toBeNull();
    }

    fireEvent.click(label);

    expect(label).toHaveAttribute('aria-expanded', 'true');
    for (const segment of screen.getAllByTestId('reasoning')) {
      expect(segment).not.toHaveAttribute('hidden');
    }
  });

  it('shows the thinking while the turn is still being written', () => {
    render(<Activity reasoning="Thinking it through." calls={[search]} live answered={false} />);

    expect(screen.getByRole('button', { name: 'Reasoning' })).toHaveAttribute(
      'aria-expanded',
      'true'
    );
    for (const segment of screen.getAllByTestId('reasoning')) {
      expect(segment).not.toHaveAttribute('hidden');
    }
  });

  it('shows the thinking behind a call that is waiting on the reader', () => {
    render(
      <Activity
        calls={[call({ name: 'run_shell', reasoning: 'This needs a shell.', approval: 'pending' })]}
        live={false}
        answered={false}
        onDecide={() => {}}
      />
    );

    expect(screen.getByTestId('reasoning')).not.toHaveAttribute('hidden');
    expect(screen.getByTestId('tool-approve').closest('[hidden]')).toBeNull();
  });

  it('lists the tool rows without a label when the turn never thought aloud', () => {
    render(<Activity calls={[call()]} live={false} answered={false} />);

    expect(screen.queryByText('Reasoning')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-call').closest('[hidden]')).toBeNull();
  });

  it('puts thinking that led to no call above the trace', () => {
    render(
      <Activity reasoning="No tool needed for this." calls={[call()]} live answered={false} />
    );

    expect(
      follows(
        screen.getByText('No tool needed for this.'),
        screen.getByText('Searched the knowledge base')
      )
    ).toBe(true);
  });
});

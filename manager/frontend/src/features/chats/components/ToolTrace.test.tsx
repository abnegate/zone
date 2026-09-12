import { describe, expect, it } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import { AWAITING_ANSWER_DETAIL, type Question, type ToolCallRecord } from '../types';
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

  it('offers no decision once the approval is closed', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            detail: 'Waiting for approval…',
          }),
        ]}
        onDecide={() => {
          throw new Error('a closed approval must not be decidable');
        }}
      />
    );

    expect(screen.queryByTestId('tool-approve')).not.toBeInTheDocument();
    expect(screen.queryByTestId('tool-deny')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-call').closest('li')).toHaveClass('tool-call--pending');
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

  it('shows what the server read the call as doing while it waits to be allowed', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'run_shell',
            arguments: '{"command":"rm -rf build"}',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
            preview: 'Run `rm -rf build` in /srv/zone.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.getByTestId('tool-call-preview')).toHaveTextContent(
      'Run `rm -rf build` in /srv/zone.'
    );
  });

  it('puts what the call will do above the buttons that allow it', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
            preview: 'Write 12 characters to config.toml, replacing whatever is there.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.getByTestId('tool-call-preview')).toBeInTheDocument();
    const order = Array.from(
      screen
        .getByTestId('tool-call')
        .closest('li')
        ?.querySelectorAll<HTMLElement>('[data-testid]') ?? []
    ).map((element) => element.dataset.testid);

    expect(order.indexOf('tool-call-preview')).toBeLessThan(order.indexOf('tool-approve'));
    expect(order.indexOf('tool-call-preview')).toBeLessThan(order.indexOf('tool-deny'));
  });

  it('marks the preview as read from the call, not as another thing the model said', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
            reason: 'The user asked me to save the config.',
            preview: 'Write 12 characters to config.toml, replacing whatever is there.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.getByTestId('tool-call-preview')).toHaveTextContent(
      'Effect, read from the call by the server'
    );
    expect(screen.getByTestId('tool-call-preview')).not.toHaveTextContent(
      'Reason, stated by the model'
    );
    expect(screen.getByTestId('tool-call-reason')).toHaveTextContent('Reason, stated by the model');
  });

  it('shows no preview at all for a call that arrived without one', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            pending: true,
            approval: 'pending',
            detail: 'Waiting for approval…',
            reason: 'The user asked me to save the config.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.queryByTestId('tool-call-preview')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-approve')).toBeInTheDocument();
  });

  it('treats a blank preview as none rather than an empty line', () => {
    render(<ToolTrace calls={[call({ name: 'apply_patch', preview: '   ' })]} />);

    expect(screen.queryByTestId('tool-call-preview')).not.toBeInTheDocument();
  });

  it('keeps the preview on the row once the reader has approved it', () => {
    render(
      <ToolTrace
        calls={[
          call({
            name: 'write_file',
            detail: 'Wrote config.toml',
            preview: 'Write 12 characters to config.toml, replacing whatever is there.',
          }),
        ]}
        onDecide={() => {}}
      />
    );

    expect(screen.getByTestId('tool-call-preview')).toHaveTextContent(
      'Write 12 characters to config.toml, replacing whatever is there.'
    );
    expect(screen.queryByTestId('tool-approve')).not.toBeInTheDocument();
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

  const scope: Question = {
    header: 'Scope',
    question: 'How far should this go?',
    choices: [
      {
        label: 'Backfill',
        description: 'Rewrite every existing row.',
        recommended: true,
        free_text: false,
      },
      {
        label: 'Forward only',
        description: 'Leave the existing rows alone.',
        recommended: false,
        free_text: false,
      },
      {
        label: 'Other',
        description: 'Something else — type it below.',
        recommended: false,
        free_text: true,
      },
    ],
    multi_select: false,
    required: true,
  };

  const asked = (questions: Question[] = [scope]): ToolCallRecord =>
    call({
      name: 'ask_user',
      arguments: '{"questions":[{"header":"Scope"}]}',
      detail: AWAITING_ANSWER_DETAIL,
      duration_ms: 0,
      questions,
    });

  it('describes an asked question in plain language', () => {
    render(<ToolTrace calls={[asked()]} />);

    expect(screen.getByText('Asked you a question')).toBeInTheDocument();
    expect(screen.queryByText('ask_user')).not.toBeInTheDocument();
  });

  it('shows a question loaded from history without the row being opened', () => {
    render(<ToolTrace calls={[asked()]} onAnswer={() => {}} />);

    expect(screen.getByTestId('question-card')).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeInTheDocument();
    expect(screen.getByTestId('tool-call')).toHaveAttribute('aria-expanded', 'false');
  });

  it('leaves the trace alone for a call that asked nothing', () => {
    render(<ToolTrace calls={[call()]} onAnswer={() => {}} />);

    expect(screen.queryByTestId('question-card')).not.toBeInTheDocument();
  });

  it('sends the answer as the agreed rendering rather than as raw choices', () => {
    const sent: string[] = [];
    render(<ToolTrace calls={[asked()]} onAnswer={(content) => sent.push(content)} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Other' }));
    fireEvent.change(screen.getByTestId('question-free-text'), {
      target: { value: 'Only the backlog' },
    });
    fireEvent.click(screen.getByTestId('question-submit'));

    expect(sent).toEqual(['Scope: Other: Only the backlog']);
  });

  // A user message newer than the assistant message the card sits on is the
  // reader's answer, so the caller reads the settled state off the thread.
  it('settles the card once a later user message has answered it', () => {
    render(<ToolTrace calls={[asked()]} answered onAnswer={() => {}} />);

    expect(screen.getByTestId('question-submit')).toBeDisabled();
    expect(screen.getByTestId('question-card')).toHaveClass('question-card--answered');
  });

  it('still asks while no answer has been sent', () => {
    render(<ToolTrace calls={[asked()]} onAnswer={() => {}} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));

    expect(screen.getByTestId('question-submit')).toBeEnabled();
  });

  it('keeps the questions beside the reasons rather than behind the toggle', () => {
    render(
      <ToolTrace
        calls={[asked([{ ...scope, preview: 'Decides whether 4,812 rows are rewritten.' }])]}
        onAnswer={() => {}}
      />
    );

    const order = Array.from(
      screen
        .getByTestId('tool-call')
        .closest('li')
        ?.querySelectorAll<HTMLElement>('[data-testid]') ?? []
    ).map((element) => element.dataset.testid);

    expect(order.indexOf('question-card')).toBeGreaterThan(order.indexOf('tool-call'));
    expect(screen.getByTestId('question-preview')).toHaveTextContent(
      'Decides whether 4,812 rows are rewritten.'
    );
  });
});

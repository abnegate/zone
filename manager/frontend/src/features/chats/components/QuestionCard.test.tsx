import { describe, expect, it, mock } from 'bun:test';
import { fireEvent, render, screen } from '@testing-library/react';
import type { Answer, Choice, Question } from '../types';
import { QuestionCard } from './QuestionCard';

const choice = (label: string, overrides: Partial<Choice> = {}): Choice => ({
  label,
  description: `${label} described`,
  recommended: false,
  free_text: false,
  ...overrides,
});

const question = (overrides: Partial<Question> = {}): Question => ({
  header: 'Scope',
  question: 'How far should this go?',
  choices: [
    choice('Backfill', { recommended: true, description: 'Rewrite every existing row.' }),
    choice('Forward only', { description: 'Leave the existing rows alone.' }),
    choice('Other', { free_text: true, description: 'Something else — type it below.' }),
  ],
  multi_select: false,
  required: true,
  ...overrides,
});

const submit = () => screen.getByTestId('question-submit');

const describedBy = (input: HTMLElement): string[] =>
  (input.getAttribute('aria-describedby') ?? '')
    .split(/\s+/)
    .filter(Boolean)
    .map((id) => document.getElementById(id)?.textContent ?? '');

describe('QuestionCard', () => {
  it('renders nothing when the call carried no questions', () => {
    const { container } = render(
      <QuestionCard questions={[]} answered={false} onSubmit={() => {}} />
    );

    expect(container.firstChild).toBeNull();
  });

  it("asks the question in the agent's own words under its header", () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(screen.getByText('Scope')).toBeInTheDocument();
    expect(screen.getByText('How far should this go?')).toBeInTheDocument();
  });

  it('shows every choice with the description that distinguishes it', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeInTheDocument();
    expect(screen.getByText('Rewrite every existing row.')).toBeInTheDocument();
    expect(screen.getByText('Leave the existing rows alone.')).toBeInTheDocument();
  });

  it('marks the recommended choice, which the server puts first', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    const marks = screen.getAllByTestId('question-recommended');
    expect(marks).toHaveLength(1);
    expect(marks[0].closest('li')).toHaveTextContent('Backfill');
    expect(screen.getAllByRole('radio')[0]).toBe(screen.getByRole('radio', { name: 'Backfill' }));
  });

  it('tells assistive technology which choice is recommended', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(describedBy(screen.getByRole('radio', { name: 'Backfill' }))).toEqual([
      'Recommended',
      'Rewrite every existing row.',
    ]);
    expect(describedBy(screen.getByRole('radio', { name: 'Forward only' }))).toEqual([
      'Leave the existing rows alone.',
    ]);
  });

  it('says whether an answer is required or optional', () => {
    render(
      <QuestionCard
        questions={[question(), question({ header: 'Notify', required: false })]}
        answered={false}
        onSubmit={() => {}}
      />
    );

    const markers = screen.getAllByTestId('question-marker');
    expect(markers[0]).toHaveTextContent('Required');
    expect(markers[1]).toHaveTextContent('Optional');
  });

  it('offers radios for a single-select question and keeps one answer', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));
    fireEvent.click(screen.getByRole('radio', { name: 'Forward only' }));

    expect(screen.getByRole('radio', { name: 'Backfill' })).not.toBeChecked();
    expect(screen.getByRole('radio', { name: 'Forward only' })).toBeChecked();
  });

  it('offers checkboxes for a multi-select question and keeps both answers', () => {
    render(
      <QuestionCard
        questions={[question({ multi_select: true })]}
        answered={false}
        onSubmit={() => {}}
      />
    );

    fireEvent.click(screen.getByRole('checkbox', { name: 'Backfill' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Forward only' }));

    expect(screen.getByRole('checkbox', { name: 'Backfill' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'Forward only' })).toBeChecked();
  });

  it('keeps the free-text box shut until its own choice is picked', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(screen.getByTestId('question-free-text')).toBeDisabled();

    fireEvent.click(screen.getByRole('radio', { name: 'Other' }));

    expect(screen.getByTestId('question-free-text')).toBeEnabled();
  });

  it('will not send while a required question is unanswered', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(submit()).toBeDisabled();

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));

    expect(submit()).toBeEnabled();
  });

  it('lets an optional question be skipped once another one is answered', () => {
    render(
      <QuestionCard
        questions={[question(), question({ header: 'Notify', required: false })]}
        answered={false}
        onSubmit={() => {}}
      />
    );

    fireEvent.click(screen.getAllByRole('radio', { name: 'Backfill' })[0]);

    expect(submit()).toBeEnabled();
  });

  it('will not send a card nothing has been chosen on, optional though it all is', () => {
    const onSubmit = mock((_answers: Answer[]) => {});
    render(
      <QuestionCard
        questions={[question({ required: false }), question({ header: 'Notify', required: false })]}
        answered={false}
        onSubmit={onSubmit}
      />
    );

    expect(submit()).toBeDisabled();

    fireEvent.click(screen.getAllByRole('radio', { name: 'Backfill' })[1]);

    expect(submit()).toBeEnabled();
    fireEvent.click(submit());
    expect(onSubmit).toHaveBeenCalledWith([{ header: 'Notify', labels: ['Backfill'] }]);
  });

  it('will not send an empty answer typed into the free-text box', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    fireEvent.click(screen.getByRole('radio', { name: 'Other' }));
    expect(submit()).toBeDisabled();

    fireEvent.change(screen.getByTestId('question-free-text'), { target: { value: '   ' } });
    expect(submit()).toBeDisabled();

    fireEvent.change(screen.getByTestId('question-free-text'), {
      target: { value: 'Only the backlog' },
    });
    expect(submit()).toBeEnabled();
  });

  it('shows what the server read the answer as deciding', () => {
    render(
      <QuestionCard
        questions={[question({ preview: 'Rewrites 4,812 rows in place.' })]}
        answered={false}
        onSubmit={() => {}}
      />
    );

    const preview = screen.getByTestId('question-preview');
    expect(preview).toHaveTextContent('Rewrites 4,812 rows in place.');
    expect(preview).toHaveTextContent('Effect, read from the call by the server');
  });

  it('shows no preview at all for a question that arrived without one', () => {
    render(<QuestionCard questions={[question()]} answered={false} onSubmit={() => {}} />);

    expect(screen.queryByTestId('question-preview')).not.toBeInTheDocument();
  });

  it('settles once the reader has already answered', () => {
    render(<QuestionCard questions={[question()]} answered onSubmit={() => {}} />);

    expect(submit()).toBeDisabled();
    expect(submit()).toHaveTextContent('Answered');
    expect(screen.getByRole('radio', { name: 'Backfill' })).toBeDisabled();
    expect(screen.getByTestId('question-card')).toHaveClass('question-card--answered');
  });

  it('hands back the structured answers, free text and all', () => {
    const sent: Answer[][] = [];
    render(
      <QuestionCard
        questions={[
          question(),
          question({
            header: 'Rollout',
            question: 'How fast?',
            multi_select: true,
            required: false,
            choices: [
              choice('All at once', { recommended: true }),
              choice('Per workspace'),
              choice('Other', { free_text: true }),
            ],
          }),
        ]}
        answered={false}
        onSubmit={(answers) => sent.push(answers)}
      />
    );

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Per workspace' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Other' }));
    fireEvent.change(screen.getByLabelText('Rollout: Other'), {
      target: { value: '  Two workspaces first, then the rest  ' },
    });
    fireEvent.click(submit());

    expect(sent).toEqual([
      [
        { header: 'Scope', labels: ['Backfill'] },
        {
          header: 'Rollout',
          labels: ['Per workspace', 'Other'],
          other: 'Two workspaces first, then the rest',
        },
      ],
    ]);
  });

  it('leaves an unanswered optional question out of the answers entirely', () => {
    const sent: Answer[][] = [];
    render(
      <QuestionCard
        questions={[
          question(),
          question({
            header: 'Notify',
            required: false,
            choices: [choice('Yes'), choice('No'), choice('Other', { free_text: true })],
          }),
        ]}
        answered={false}
        onSubmit={(answers) => sent.push(answers)}
      />
    );

    fireEvent.click(screen.getByRole('radio', { name: 'Backfill' }));
    fireEvent.click(submit());

    expect(sent).toEqual([[{ header: 'Scope', labels: ['Backfill'] }]]);
  });

  it('orders the labels of a multi-select answer as the agent listed them', () => {
    const sent: Answer[][] = [];
    render(
      <QuestionCard
        questions={[question({ multi_select: true })]}
        answered={false}
        onSubmit={(answers) => sent.push(answers)}
      />
    );

    fireEvent.click(screen.getByRole('checkbox', { name: 'Forward only' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Backfill' }));
    fireEvent.click(submit());

    expect(sent).toEqual([[{ header: 'Scope', labels: ['Backfill', 'Forward only'] }]]);
  });
});

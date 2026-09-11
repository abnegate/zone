import { describe, expect, it } from 'bun:test';
import type { Answer, Question } from '../types';
import { renderAnswers } from './answers';

const question = (
  header: string,
  labels: string[],
  overrides: Partial<Question> = {}
): Question => ({
  header,
  question: `${header}?`,
  choices: labels.map((label, index) => ({
    label,
    description: `${label} described`,
    recommended: index === 0,
    free_text: label === 'Other',
  })),
  multi_select: false,
  required: false,
  ...overrides,
});

const questions: Question[] = [
  question('Scope', ['Backfill', 'Forward only', 'Other'], { required: true }),
  question('Rollout', ['All at once', 'Per workspace', 'Other'], { multi_select: true }),
  question('Notify', ['Yes', 'No', 'Other']),
];

describe('renderAnswers', () => {
  // The server renders the same answer from the same input and asserts this
  // exact string. Changing either side alone is a silent divergence between
  // what the reader is shown and what the agent is told.
  it('renders the agreed fixture byte for byte', () => {
    const answers: Answer[] = [
      { header: 'Scope', labels: ['Backfill'] },
      {
        header: 'Rollout',
        labels: ['Per workspace', 'Other'],
        other: 'Two workspaces first, then the rest',
      },
    ];

    expect(renderAnswers(questions, answers)).toBe(
      'Scope: Backfill\nRollout: Per workspace, Other: Two workspaces first, then the rest'
    );
  });

  it('gives an unanswered optional question no line at all', () => {
    const rendered = renderAnswers(questions, [{ header: 'Scope', labels: ['Backfill'] }]);

    expect(rendered).toBe('Scope: Backfill');
    expect(rendered).not.toContain('Rollout');
    expect(rendered).not.toContain('Notify');
  });

  it('treats a question answered with no labels as unanswered', () => {
    expect(
      renderAnswers(questions, [
        { header: 'Scope', labels: ['Backfill'] },
        { header: 'Notify', labels: [] },
      ])
    ).toBe('Scope: Backfill');
  });

  it('writes the lines in question order however the answers arrive', () => {
    const answers: Answer[] = [
      { header: 'Notify', labels: ['No'] },
      { header: 'Scope', labels: ['Forward only'] },
    ];

    expect(renderAnswers(questions, answers)).toBe('Scope: Forward only\nNotify: No');
  });

  it('substitutes the typed text in place of the free-text label', () => {
    expect(
      renderAnswers(questions, [{ header: 'Scope', labels: ['Other'], other: 'Only the backlog' }])
    ).toBe('Scope: Other: Only the backlog');
  });

  it('ignores typed text for an answer that did not choose the free-text option', () => {
    expect(
      renderAnswers(questions, [{ header: 'Scope', labels: ['Backfill'], other: 'stale draft' }])
    ).toBe('Scope: Backfill');
  });

  it('renders nothing when no question was answered', () => {
    expect(renderAnswers(questions, [])).toBe('');
  });

  it('ignores an answer to a question that was never asked', () => {
    expect(
      renderAnswers(questions, [
        { header: 'Scope', labels: ['Backfill'] },
        { header: 'Budget', labels: ['Unlimited'] },
      ])
    ).toBe('Scope: Backfill');
  });
});

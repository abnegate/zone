import { type Answer, FREE_TEXT_LABEL, type Question } from '../types';

function renderLabel(label: string, other: string | undefined): string {
  return label === FREE_TEXT_LABEL ? `${FREE_TEXT_LABEL}: ${other ?? ''}` : label;
}

/**
 * The answer as the agent will read it back.
 *
 * An answer travels as an ordinary user message rather than a frame of its own,
 * so this string is the whole of what the model receives. The server renders the
 * same lines from the same answer, and both sides assert the same fixture: a
 * drift here would have the console showing one answer and the agent acting on
 * another. Question order rather than answer order, because that is the order
 * the reader was asked in; an unanswered optional question contributes nothing,
 * so silence stays silence instead of becoming an empty choice.
 */
export function renderAnswers(questions: Question[], answers: Answer[]): string {
  const byHeader = new Map(answers.map((answer) => [answer.header, answer]));

  return questions
    .map((question) => byHeader.get(question.header))
    .filter((answer): answer is Answer => Boolean(answer && answer.labels.length > 0))
    .map(
      (answer) =>
        `${answer.header}: ${answer.labels.map((label) => renderLabel(label, answer.other)).join(', ')}`
    )
    .join('\n');
}

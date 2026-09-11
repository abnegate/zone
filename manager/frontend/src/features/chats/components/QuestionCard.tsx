import { useId, useState } from 'react';
import { type Answer, PREVIEW_LABEL, type Question } from '../types';
import './QuestionCard.css';

const SUBMIT_LABEL = 'Send answer';
const SETTLED_LABEL = 'Answered';
const REQUIRED_LABEL = 'Required';
const OPTIONAL_LABEL = 'Optional';
const RECOMMENDED_LABEL = 'Recommended';

type Selections = Record<string, string[]>;
type Texts = Record<string, string>;

function freeTextChoice(question: Question) {
  return question.choices.find((choice) => choice.free_text);
}

function chosenLabels(question: Question, selections: Selections): string[] {
  const chosen = selections[question.header] ?? [];
  return question.choices
    .filter((choice) => chosen.includes(choice.label))
    .map((choice) => choice.label);
}

function unanswered(questions: Question[], selections: Selections, texts: Texts): boolean {
  return questions.some((question) => {
    const chosen = chosenLabels(question, selections);
    if (question.required && chosen.length === 0) return true;
    const free = freeTextChoice(question);
    return Boolean(free && chosen.includes(free.label) && !(texts[question.header] ?? '').trim());
  });
}

export function QuestionCard({
  questions,
  answered,
  onSubmit,
}: {
  questions: Question[];
  answered: boolean;
  onSubmit: (answers: Answer[]) => void;
}) {
  const cardId = useId();
  const [selections, setSelections] = useState<Selections>({});
  const [texts, setTexts] = useState<Texts>({});

  if (questions.length === 0) return null;

  const choose = (question: Question, label: string, checked: boolean) => {
    setSelections((previous) => {
      const current = previous[question.header] ?? [];
      if (!question.multi_select) {
        return { ...previous, [question.header]: [label] };
      }
      return {
        ...previous,
        [question.header]: checked
          ? [...current, label]
          : current.filter((chosen) => chosen !== label),
      };
    });
  };

  const submit = () => {
    const answers = questions.flatMap<Answer>((question) => {
      const labels = chosenLabels(question, selections);
      if (labels.length === 0) return [];
      const free = freeTextChoice(question);
      const other =
        free && labels.includes(free.label) ? texts[question.header]?.trim() : undefined;
      return [
        other ? { header: question.header, labels, other } : { header: question.header, labels },
      ];
    });
    onSubmit(answers);
  };

  const blocked = answered || unanswered(questions, selections, texts);

  return (
    <div
      className={`question-card${answered ? ' question-card--answered' : ''}`}
      data-testid="question-card"
    >
      {questions.map((question, questionIndex) => {
        const group = `${cardId}-${questionIndex}`;
        const chosen = selections[question.header] ?? [];
        return (
          <fieldset className="question-card-question" key={question.header}>
            <legend className="question-card-header">
              <span className="question-card-header-text">{question.header}</span>
              <span className="question-card-marker" data-testid="question-marker">
                {question.required ? REQUIRED_LABEL : OPTIONAL_LABEL}
              </span>
            </legend>
            <p className="question-card-prompt">{question.question}</p>
            {question.preview?.trim() ? (
              <p className="tool-call-preview" data-testid="question-preview">
                <span className="tool-call-preview-label">{PREVIEW_LABEL}</span>
                <span className="tool-call-preview-text">{question.preview}</span>
              </p>
            ) : null}
            <ul className="question-card-choices">
              {question.choices.map((choice, choiceIndex) => {
                const inputId = `${group}-${choiceIndex}`;
                const descriptionId = `${inputId}-description`;
                const selected = chosen.includes(choice.label);
                return (
                  <li className="question-card-choice" key={choice.label}>
                    <input
                      type={question.multi_select ? 'checkbox' : 'radio'}
                      id={inputId}
                      name={group}
                      className="question-card-control"
                      checked={selected}
                      disabled={answered}
                      aria-describedby={descriptionId}
                      onChange={(event) => choose(question, choice.label, event.target.checked)}
                    />
                    <span className="question-card-choice-heading">
                      <label className="question-card-choice-label" htmlFor={inputId}>
                        {choice.label}
                      </label>
                      {choice.recommended && (
                        <span
                          className="question-card-recommended"
                          data-testid="question-recommended"
                        >
                          {RECOMMENDED_LABEL}
                        </span>
                      )}
                    </span>
                    <p className="question-card-choice-description" id={descriptionId}>
                      {choice.description}
                    </p>
                    {choice.free_text && (
                      <input
                        type="text"
                        className="question-card-text"
                        data-testid="question-free-text"
                        value={texts[question.header] ?? ''}
                        disabled={answered || !selected}
                        aria-label={`${question.header}: ${choice.label}`}
                        onChange={(event) =>
                          setTexts((previous) => ({
                            ...previous,
                            [question.header]: event.target.value,
                          }))
                        }
                      />
                    )}
                  </li>
                );
              })}
            </ul>
          </fieldset>
        );
      })}
      <button
        type="button"
        className="question-card-submit"
        data-testid="question-submit"
        disabled={blocked}
        onClick={submit}
      >
        {answered ? SETTLED_LABEL : SUBMIT_LABEL}
      </button>
    </div>
  );
}

import { useState } from 'react';
import { tasksApi } from '../../../api/tasks';
import { QuestionCard } from '../../chats/components';
import type { Answer } from '../../chats/types';
import type { TaskRun } from '../types';
import './QuestionPrompt.css';

const TITLE = 'The run needs an answer';
const OPTIONAL_NOTICE =
  'Leave an optional question alone and the run carries on with the recommended option after about thirty seconds.';
const REQUIRED_NOTICE =
  'A required question holds the run until someone answers it, and the run times out after an hour. A timed-out run is not retried — start a new run to try again.';
const FAILURE_TITLE = 'Could not send this answer';
const FAILURE_FALLBACK = 'The answer was not accepted.';

export function QuestionPrompt({ run, onAnswered }: { run: TaskRun; onAnswered: () => void }) {
  const [failure, setFailure] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const pending = run.pending_question;

  if (!pending) return null;

  const submit = async (answers: Answer[]): Promise<void> => {
    if (sending) return;
    setSending(true);
    setFailure(null);
    try {
      await tasksApi.answerRun(run.id, answers);
      onAnswered();
    } catch (rejection) {
      setFailure(rejection instanceof Error ? rejection.message : FAILURE_FALLBACK);
    } finally {
      setSending(false);
    }
  };

  return (
    <section className="execution-question" aria-label={TITLE}>
      <h3>{TITLE}</h3>
      <QuestionCard
        questions={pending.questions}
        answered={run.status !== 'waiting' || sending}
        onSubmit={(answers) => void submit(answers)}
      />
      {failure && (
        <div className="execution-notice" role="alert">
          <strong>{FAILURE_TITLE}</strong>
          <p>{failure}</p>
        </div>
      )}
      <p className="execution-hint">{OPTIONAL_NOTICE}</p>
      <p className="execution-hint">{REQUIRED_NOTICE}</p>
    </section>
  );
}

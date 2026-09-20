import { Button } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { getErrors } from '../../../validation';
import { AutoProjectRequestSchema } from '../schemas';
import type { AutoProjectRequest, AutoProjectResponse } from '../types';

interface AutoProjectModalProps {
  isOpen: boolean;
  onClose: () => void;
  /** Opens the interview; resolves with the planner chat to go to. */
  start: (request: AutoProjectRequest) => Promise<AutoProjectResponse>;
  onStarted: (chatId: string) => void;
}

const PLACEHOLDER =
  'A recipe app for iOS and Android with a shared backend. Users save recipes, plan a week ' +
  'of meals and get a shopping list. Calm, warm look; subtle animations.';

/**
 * The one field an auto project starts from. Everything else -- platforms,
 * stack, repository, design, tests, CI, deployment -- is asked in the chat
 * this opens, so the modal asks for nothing it would only have to ask again.
 */
export function AutoProjectModal({ isOpen, onClose, start, onStarted }: AutoProjectModalProps) {
  const briefId = useId();
  const [brief, setBrief] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  if (!isOpen) return null;

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const request: AutoProjectRequest = { brief: brief.trim() };
    const errors = getErrors(AutoProjectRequestSchema, request);
    if (Object.keys(errors).length > 0) {
      setError(errors.brief ?? 'Describe the project first');
      return;
    }
    setError(null);
    setSubmitting(true);
    try {
      const started = await start(request);
      setBrief('');
      onStarted(started.chat_id);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not start the project');
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="modal" data-testid="auto-project-modal">
      <div
        className="modal-backdrop"
        onClick={onClose}
        onKeyDown={(e) => e.key === 'Escape' && onClose()}
        role="button"
        tabIndex={0}
        aria-label="Close modal"
      />
      <div className="modal-content">
        <h3>Auto project</h3>
        <p className="auto-project-intro">
          Describe what you want built. Zone interviews you in a chat until every task can be
          written without guessing, then creates the project and its tasks and runs them: each
          change is checked, reviewed by a second model, fixed, merged and reported.
        </p>
        <form onSubmit={handleSubmit}>
          <div className="form-group">
            <label htmlFor={briefId}>Brief</label>
            <textarea
              id={briefId}
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              placeholder={PLACEHOLDER}
              rows={6}
              className={error ? 'input-error' : ''}
              data-testid="auto-project-brief"
            />
            {error && <span className="field-error">{error}</span>}
          </div>
          <div className="modal-actions">
            <Button type="button" variant="secondary" onClick={onClose} disabled={submitting}>
              Cancel
            </Button>
            <Button type="submit" disabled={submitting || !brief.trim()}>
              {submitting ? 'Opening the interview…' : 'Start the interview'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}

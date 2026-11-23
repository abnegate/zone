import { Button, Input } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { licenseDate } from '../../status';
import { useUpdateFleetPolicy } from '../hooks';
import {
  CLEAR_MINIMUM,
  failureText,
  MINIMUM_DESCRIPTION,
  MINIMUM_LABEL,
  MINIMUM_PLACEHOLDER,
  MINIMUM_TITLE,
  NO_MINIMUM,
  SAVE_MINIMUM,
  UPDATED_LABEL,
} from '../messages';
import type { FleetPolicyResponse, UpdateFleetPolicyRequest } from '../types';
import { NoRunnerLeftDialog } from './NoRunnerLeftDialog';

interface MinimumVersionFieldProps {
  policy: FleetPolicyResponse;
}

function minimumOf(text: string): string | null {
  const trimmed = text.trim();
  return trimmed === '' ? null : trimmed;
}

export function MinimumVersionField({ policy }: MinimumVersionFieldProps) {
  const titleId = useId();
  const inputId = useId();
  const errorId = useId();
  const { update, pending, refusal, error, reset } = useUpdateFleetPolicy();
  const saved = policy.minimum_version;
  const [text, setText] = useState(saved ?? '');
  const [confirming, setConfirming] = useState<UpdateFleetPolicyRequest | null>(null);
  const wanted = minimumOf(text);
  const dirty = wanted !== saved;
  const failure = refusal?.leavesNoRunner ? null : failureText(refusal, error);

  const send = (request: UpdateFleetPolicyRequest) =>
    update(
      request,
      (next) => {
        setConfirming(null);
        setText(next.minimum_version ?? '');
      },
      (refused) => {
        if (refused.leavesNoRunner) setConfirming({ ...request, confirm: true });
      }
    );

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || !dirty || !policy.editable) {
      return;
    }
    send({ minimum_version: wanted });
  };

  const dismiss = () => {
    setConfirming(null);
    reset();
  };

  return (
    <section className="settings-card runners-minimum" aria-labelledby={titleId}>
      <h3 id={titleId} className="runners-card-title">
        {MINIMUM_TITLE}
      </h3>
      <p className="runners-hint">{MINIMUM_DESCRIPTION}</p>
      {policy.updated_at && (
        <p className="runners-hint">
          {UPDATED_LABEL} {licenseDate(policy.updated_at)}
        </p>
      )}
      {policy.editable ? (
        <form className="runners-minimum-form" onSubmit={submit} noValidate>
          <Input
            id={inputId}
            className="runners-minimum-input"
            aria-label={MINIMUM_LABEL}
            aria-invalid={failure ? true : undefined}
            aria-describedby={failure && !confirming ? errorId : undefined}
            value={text}
            autoComplete="off"
            spellCheck={false}
            inputMode="decimal"
            placeholder={MINIMUM_PLACEHOLDER}
            disabled={pending}
            onChange={(event) => {
              setText(event.target.value);
              if (error) reset();
            }}
          />
          <div className="runners-minimum-actions">
            {saved !== null && (
              <Button
                type="button"
                variant="ghost"
                disabled={pending}
                onClick={() => {
                  setText('');
                  send({ minimum_version: null });
                }}
              >
                {CLEAR_MINIMUM}
              </Button>
            )}
            <Button type="submit" loading={pending} disabled={!dirty}>
              {SAVE_MINIMUM}
            </Button>
          </div>
        </form>
      ) : (
        <p className="runners-minimum-value">{saved ?? NO_MINIMUM}</p>
      )}
      {policy.editable && saved === null && !dirty && <p className="runners-hint">{NO_MINIMUM}</p>}
      {failure && !confirming && (
        <p id={errorId} className="runners-error" role="alert">
          {failure}
        </p>
      )}
      {confirming && (
        <NoRunnerLeftDialog
          pending={pending}
          error={failure}
          onConfirm={() => send(confirming)}
          onClose={dismiss}
        />
      )}
    </section>
  );
}

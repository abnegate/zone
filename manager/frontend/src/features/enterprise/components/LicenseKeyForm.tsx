import { Button } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { useSetLicense } from '../hooks';
import { refusalMessage } from '../status';

interface LicenseKeyFormProps {
  licensed: boolean;
}

export function LicenseKeyForm({ licensed }: LicenseKeyFormProps) {
  const fieldId = useId();
  const hintId = useId();
  const errorId = useId();
  const { set, pending, refusal, error, reset } = useSetLicense();
  const [key, setKey] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  const trimmed = key.trim();
  const failure = refusal ? refusalMessage(refusal.code) : error;
  const describedBy = failure ? `${hintId} ${errorId}` : hintId;

  const change = (value: string) => {
    setKey(value);
    setNotice(null);
    if (failure) {
      reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (trimmed === '' || pending) {
      return;
    }
    const saved = licensed ? 'License replaced.' : 'License added.';
    set(trimmed, () => {
      setKey('');
      setNotice(saved);
    });
  };

  return (
    <form className="settings-card license-key-form" onSubmit={submit}>
      <div className="form-group">
        <label htmlFor={fieldId}>License key</label>
        <textarea
          id={fieldId}
          className="license-key-input"
          rows={4}
          value={key}
          spellCheck={false}
          autoComplete="off"
          disabled={pending}
          aria-invalid={failure ? true : undefined}
          aria-describedby={describedBy}
          onChange={(event) => change(event.target.value)}
        />
        <p id={hintId} className="form-hint">
          Paste the key exactly as you received it. Line breaks from email are fine.
        </p>
        {failure && (
          <p id={errorId} className="field-error" role="alert">
            {failure}
          </p>
        )}
      </div>
      <div className="license-key-actions">
        {notice && (
          <span className="license-key-notice" role="status">
            {notice}
          </span>
        )}
        <Button type="submit" disabled={trimmed === ''} loading={pending}>
          {licensed ? 'Replace license' : 'Add license'}
        </Button>
      </div>
    </form>
  );
}

import { Button } from '@zone/ui';
import { useId } from 'react';
import { DISMISS_LABEL, SECRET_HINT, SECRET_NAME, secretTitle } from '../labels';
import type { IssuedResponse } from '../types';
import { CopyValue } from './CopyValue';

interface ScimTokenSecretProps {
  issued: IssuedResponse;
  onDismiss: () => void;
}

export function ScimTokenSecret({ issued, onDismiss }: ScimTokenSecretProps) {
  const headingId = useId();

  return (
    <section className="settings-card scim-card scim-secret" aria-labelledby={headingId}>
      <div className="scim-card-header">
        <h3 id={headingId} className="scim-card-title">
          {secretTitle(issued.token.name)}
        </h3>
        <Button type="button" variant="ghost" size="sm" onClick={onDismiss}>
          {DISMISS_LABEL}
        </Button>
      </div>
      <CopyValue value={issued.secret} name={SECRET_NAME} />
      <p className="scim-hint">{SECRET_HINT}</p>
    </section>
  );
}

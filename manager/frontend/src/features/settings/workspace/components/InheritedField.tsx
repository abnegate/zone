import { Button } from '@zone/ui';
import type { ReactNode } from 'react';
import { FieldSource } from '../inheritance';
import './InheritedField.css';

const BADGES: Record<FieldSource, string> = {
  [FieldSource.Organization]: 'Inherited from organization',
  [FieldSource.Workspace]: 'Set for this workspace',
};

interface InheritedFieldProps {
  label: string;
  value: string;
  source: FieldSource;
  editable: boolean;
  editing: boolean;
  busy: boolean;
  onOverride(): void;
  onReset(): void;
  children?: ReactNode;
}

export default function InheritedField({
  label,
  value,
  source,
  editable,
  editing,
  busy,
  onOverride,
  onReset,
  children,
}: InheritedFieldProps) {
  return (
    <div className="inherited-field" role="group" aria-label={label}>
      <div className="inherited-field-row">
        <span className="inherited-field-label">{label}</span>
        <span className="inherited-field-value">{value}</span>
        <span className={`inherited-field-badge inherited-field-badge--${source}`}>
          {BADGES[source]}
        </span>
        {editable && !editing && (
          <div className="inherited-field-actions">
            {source === FieldSource.Organization ? (
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Override ${label}`}
                disabled={busy}
                onClick={onOverride}
              >
                Override
              </Button>
            ) : (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`Edit ${label}`}
                  disabled={busy}
                  onClick={onOverride}
                >
                  Edit
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  aria-label={`Reset ${label}`}
                  disabled={busy}
                  onClick={onReset}
                >
                  Reset
                </Button>
              </>
            )}
          </div>
        )}
      </div>
      {editing && <div className="inherited-field-editor">{children}</div>}
    </div>
  );
}

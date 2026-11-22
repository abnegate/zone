import { Label } from '@zone/ui';
import { DESCRIPTION_HINT, DESCRIPTION_LABEL } from '../labels';

const DESCRIPTION_ROWS = 3;

interface RoleDescriptionProps {
  id: string;
  value: string;
  editable: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}

export function RoleDescription({ id, value, editable, disabled, onChange }: RoleDescriptionProps) {
  if (!editable) {
    return value.trim() === '' ? null : (
      <div className="roles-description-fixed">
        <span className="roles-group-title">{DESCRIPTION_LABEL}</span>
        <p className="roles-description-text">{value}</p>
      </div>
    );
  }

  const hintId = `${id}-hint`;
  return (
    <div className="ui-input-wrapper">
      <Label htmlFor={id}>{DESCRIPTION_LABEL}</Label>
      <textarea
        id={id}
        className="roles-description-input"
        rows={DESCRIPTION_ROWS}
        value={value}
        autoComplete="off"
        disabled={disabled}
        aria-describedby={hintId}
        onChange={(event) => onChange(event.target.value)}
      />
      <p id={hintId} className="ui-input-help-text">
        {DESCRIPTION_HINT}
      </p>
    </div>
  );
}

import { useId } from 'react';
import { ceilingAllowed } from '../draft';
import { ceilingLabel, fieldDescription, fieldLabel, NOT_ALLOWED } from '../messages';
import { POLICY_CEILINGS, PolicyField } from '../schemas';
import { layerSource, PolicyScope } from '../source';
import type { PolicyTier } from '../types';
import { PolicyFieldFrame } from './PolicyFieldFrame';
import { INHERIT_LABEL } from './PolicyListField';

const CHOICES: readonly (PolicyTier | null)[] = [null, ...POLICY_CEILINGS];

const UNSET = 'none';

interface PolicyCeilingFieldProps {
  scope: PolicyScope;
  value: PolicyTier | null;
  organization?: PolicyTier | null;
  effective?: PolicyTier | null;
  editable: boolean;
  error?: string;
  onChange(value: PolicyTier | null): void;
}

export function PolicyCeilingField({
  scope,
  value,
  organization = null,
  effective = null,
  editable,
  error,
  onChange,
}: PolicyCeilingFieldProps) {
  const name = useId();
  const workspace = scope === PolicyScope.Workspace;
  const label = fieldLabel(PolicyField.Ceiling);

  const choiceLabel = (choice: PolicyTier | null): string =>
    choice === null && workspace ? INHERIT_LABEL : ceilingLabel(choice);

  return (
    <PolicyFieldFrame
      label={label}
      description={fieldDescription(PolicyField.Ceiling)}
      source={workspace ? layerSource(value !== null, organization !== null) : undefined}
      organization={workspace ? ceilingLabel(organization) : undefined}
      effective={workspace ? ceilingLabel(effective) : undefined}
      error={error}
    >
      <div className="policy-ceiling" role="radiogroup" aria-label={label}>
        {CHOICES.map((choice) => {
          const allowed = !workspace || choice === null || ceilingAllowed(organization, choice);
          const checked = value === choice;
          return (
            <label
              key={choice ?? UNSET}
              className={
                allowed
                  ? 'policy-ceiling-option'
                  : 'policy-ceiling-option policy-ceiling-option--denied'
              }
            >
              <input
                type="radio"
                name={name}
                value={choice ?? UNSET}
                checked={checked}
                disabled={!editable || (!allowed && !checked)}
                onChange={() => onChange(choice)}
              />
              <span className="policy-ceiling-copy">
                <span className="policy-ceiling-label">{choiceLabel(choice)}</span>
                {!allowed && <span className="policy-ceiling-reason">{NOT_ALLOWED}</span>}
              </span>
            </label>
          );
        })}
      </div>
    </PolicyFieldFrame>
  );
}

import { Button, Modal } from '@zone/ui';
import { useState } from 'react';
import { enabledCount } from '../draft';
import { useSetSsoEnforcement } from '../hooks';
import { refusalMessage } from '../messages';
import type { SettingsResponse } from '../types';
import { Switch } from './Switch';

export const ENFORCEMENT_LABEL = 'Require single sign-on';

export const ENFORCEMENT_DESCRIPTION =
  'Password sign-in, registration and password reset are turned off for everyone except server admins.';

export const ENFORCEMENT_BLOCKED = 'Add and enable a provider before requiring single sign-on.';

export const ENFORCED_READ_ONLY =
  'Single sign-on is required. Only server admins can sign in with a password.';

interface SsoEnforcementProps {
  settings: SettingsResponse;
  editable: boolean;
}

export function SsoEnforcement({ settings, editable }: SsoEnforcementProps) {
  const { set, pending, refusal, error, reset } = useSetSsoEnforcement();
  const [confirming, setConfirming] = useState(false);
  const failure = refusal ? refusalMessage(refusal.code, refusal.message) : error;
  const blocked = !settings.required && enabledCount(settings.providers) === 0;

  if (!editable) {
    return settings.required ? (
      <div className="settings-card sso-enforcement">
        <p className="sso-hint">{ENFORCED_READ_ONLY}</p>
      </div>
    ) : null;
  }

  const change = (required: boolean) => {
    reset();
    if (required) {
      setConfirming(true);
    } else {
      set(false);
    }
  };

  const close = () => {
    reset();
    setConfirming(false);
  };

  return (
    <div className="settings-card sso-enforcement">
      <Switch
        label={ENFORCEMENT_LABEL}
        description={ENFORCEMENT_DESCRIPTION}
        hint={blocked ? ENFORCEMENT_BLOCKED : undefined}
        checked={settings.required}
        disabled={pending || blocked}
        onChange={change}
      />
      {failure && !confirming && (
        <p className="field-error" role="alert">
          {failure}
        </p>
      )}
      {confirming && (
        <Modal
          isOpen
          onClose={pending ? undefined : close}
          title="Require single sign-on?"
          size="sm"
        >
          <div className="sso-dialog">
            <p>
              Everyone except server admins must sign in with an identity provider. Password
              sign-in, registration and password reset stop working for them. People already signed
              in stay signed in until their session ends.
            </p>
            {failure && (
              <div className="modal-error" role="alert">
                {failure}
              </div>
            )}
            <div className="modal-actions">
              <Button variant="ghost" disabled={pending} onClick={close}>
                Cancel
              </Button>
              <Button loading={pending} onClick={() => set(true, () => setConfirming(false))}>
                {ENFORCEMENT_LABEL}
              </Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}

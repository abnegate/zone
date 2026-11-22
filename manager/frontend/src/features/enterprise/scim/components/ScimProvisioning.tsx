import { useState } from 'react';
import { LapsedNotice } from '../../components/LapsedNotice';
import { UpgradeNotice } from '../../components/UpgradeNotice';
import { community, lapsed } from '../../policy/license';
import { useRoles } from '../../roles/hooks';
import { LAPSED_NOTICE, UPGRADE_NOTICE } from '../labels';
import { organizationRoles } from '../mapping';
import type { IssuedResponse, ScimSettings, TokenResponse } from '../types';
import { CreateScimTokenDialog } from './CreateScimTokenDialog';
import { RevokeScimTokenDialog } from './RevokeScimTokenDialog';
import { RotateScimTokenDialog } from './RotateScimTokenDialog';
import { ScimBaseUrl } from './ScimBaseUrl';
import { ScimErrorList } from './ScimErrorList';
import { ScimGroupList } from './ScimGroupList';
import { ScimTokenList } from './ScimTokenList';
import { ScimTokenSecret } from './ScimTokenSecret';

interface ScimProvisioningProps {
  organizationId: string;
  settings: ScimSettings;
}

function configured(settings: ScimSettings): boolean {
  return (
    settings.tokens.length > 0 || settings.groups.length > 0 || settings.last_synced_at !== null
  );
}

export function ScimProvisioning({ organizationId, settings }: ScimProvisioningProps) {
  const [creating, setCreating] = useState(false);
  const [rotating, setRotating] = useState<TokenResponse | null>(null);
  const [revoking, setRevoking] = useState<TokenResponse | null>(null);
  const [issued, setIssued] = useState<IssuedResponse | null>(null);
  const editable = settings.entitled;
  const { response: roles } = useRoles(editable ? organizationId : null);
  const upgrade = community(settings);
  const shown = !upgrade || configured(settings);

  const show = (next: IssuedResponse) => {
    setIssued(next);
    setCreating(false);
    setRotating(null);
  };

  const revoked = (token: TokenResponse) => {
    setRevoking(null);
    if (issued?.token.id === token.id) {
      setIssued(null);
    }
  };

  return (
    <>
      {upgrade && <UpgradeNotice message={UPGRADE_NOTICE} className="scim-notice" />}
      {lapsed(settings) && <LapsedNotice message={LAPSED_NOTICE} className="scim-lapsed" />}
      {shown && (
        <>
          <ScimBaseUrl url={settings.base_url} />
          {issued && <ScimTokenSecret issued={issued} onDismiss={() => setIssued(null)} />}
          <ScimTokenList
            tokens={settings.tokens}
            editable={editable}
            onCreate={() => setCreating(true)}
            onRotate={setRotating}
            onRevoke={setRevoking}
          />
          <ScimGroupList
            organizationId={organizationId}
            groups={settings.groups}
            offered={organizationRoles(roles)}
            editable={editable}
          />
          <ScimErrorList lastSyncedAt={settings.last_synced_at} errors={settings.errors} />
        </>
      )}
      {creating && (
        <CreateScimTokenDialog
          organizationId={organizationId}
          onIssued={show}
          onClose={() => setCreating(false)}
        />
      )}
      {rotating && (
        <RotateScimTokenDialog
          key={rotating.id}
          organizationId={organizationId}
          token={rotating}
          onIssued={show}
          onClose={() => setRotating(null)}
        />
      )}
      {revoking && (
        <RevokeScimTokenDialog
          key={revoking.id}
          organizationId={organizationId}
          token={revoking}
          onRevoked={revoked}
          onClose={() => setRevoking(null)}
        />
      )}
    </>
  );
}

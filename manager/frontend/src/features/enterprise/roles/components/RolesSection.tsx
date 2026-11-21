import { Button } from '@zone/ui';
import { useId, useState } from 'react';
import { community, lapsed } from '../../policy/license';
import { useRoles } from '../hooks';
import { DESCRIPTION, LOADING, READ_ONLY_HINT, TITLE } from '../labels';
import type { CustomRole, RolesResponse } from '../types';
import { DeleteRoleDialog } from './DeleteRoleDialog';
import { RoleEditor } from './RoleEditor';
import { RoleList } from './RoleList';
import { RolesLapsedNotice } from './RolesLapsedNotice';
import { RolesUpgradeNotice } from './RolesUpgradeNotice';
import './RolesSection.css';

interface RolesProps {
  organizationId: string;
  response: RolesResponse;
}

function Roles({ organizationId, response }: RolesProps) {
  const [creating, setCreating] = useState(false);
  const [opened, setOpened] = useState<CustomRole | null>(null);
  const [deleting, setDeleting] = useState<CustomRole | null>(null);
  const editable = response.entitled && response.editable;
  const upgrade = community(response);
  const listed = !upgrade || response.roles.length > 0;

  return (
    <>
      {upgrade && <RolesUpgradeNotice />}
      {lapsed(response) && <RolesLapsedNotice />}
      {response.entitled && !response.editable && <p className="roles-hint">{READ_ONLY_HINT}</p>}
      {listed && (
        <RoleList
          roles={response.roles}
          editable={editable}
          onCreate={() => setCreating(true)}
          onOpen={setOpened}
          onDelete={setDeleting}
        />
      )}
      {creating && (
        <RoleEditor
          organizationId={organizationId}
          catalogue={response.catalogue}
          editable={editable}
          onClose={() => setCreating(false)}
        />
      )}
      {opened && (
        <RoleEditor
          key={opened.id}
          organizationId={organizationId}
          catalogue={response.catalogue}
          role={opened}
          editable={editable}
          onClose={() => setOpened(null)}
        />
      )}
      {deleting && (
        <DeleteRoleDialog
          organizationId={organizationId}
          role={deleting}
          onClose={() => setDeleting(null)}
        />
      )}
    </>
  );
}

interface RolesSectionProps {
  organizationId: string;
}

export function RolesSection({ organizationId }: RolesSectionProps) {
  const headingId = useId();
  const { response, loading, error, refresh } = useRoles(organizationId);

  if (loading) {
    return <div className="loading-state">{LOADING}</div>;
  }

  return (
    <section className="settings-section roles-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            {TITLE}
          </h2>
          <p className="section-description">{DESCRIPTION}</p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {response && (
        <Roles key={organizationId} organizationId={organizationId} response={response} />
      )}
    </section>
  );
}

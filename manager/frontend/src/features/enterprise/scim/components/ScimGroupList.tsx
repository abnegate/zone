import type { CustomRoleReference } from '../../roles/types';
import { GROUPS_HINT, GROUPS_TITLE, META_SEPARATOR, memberCount, NO_GROUPS } from '../labels';
import type { GroupResponse } from '../types';
import { ScimGroupMapping } from './ScimGroupMapping';

interface ScimGroupListProps {
  organizationId: string;
  groups: readonly GroupResponse[];
  offered: readonly CustomRoleReference[];
  editable: boolean;
}

export function ScimGroupList({ organizationId, groups, offered, editable }: ScimGroupListProps) {
  return (
    <div className="settings-card scim-card">
      <h3 className="scim-card-title">{GROUPS_TITLE}</h3>
      <p className="scim-hint">{GROUPS_HINT}</p>
      {groups.length === 0 ? (
        <p className="scim-hint">{NO_GROUPS}</p>
      ) : (
        <ul className="scim-list">
          {groups.map((group) => (
            <li key={group.id} className="scim-item">
              <div className="scim-item-identity">
                <span className="scim-item-name">{group.display_name}</span>
                <span className="scim-item-meta">
                  {memberCount(group.members)}
                  {group.external_id !== null && (
                    <>
                      {META_SEPARATOR}
                      <code className="scim-external-id">{group.external_id}</code>
                    </>
                  )}
                </span>
              </div>
              <ScimGroupMapping
                organizationId={organizationId}
                group={group}
                offered={offered}
                editable={editable}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

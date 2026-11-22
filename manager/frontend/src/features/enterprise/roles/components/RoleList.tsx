import { Badge, Button } from '@zone/ui';
import {
  CREATE_LABEL,
  EMPTY_EDITABLE,
  EMPTY_READ_ONLY,
  LIST_TITLE,
  META_SEPARATOR,
  memberCount,
  permissionCount,
  SCOPE_LABELS,
} from '../labels';
import { RoleScope } from '../schemas';
import type { CustomRole } from '../types';

interface RoleListProps {
  roles: readonly CustomRole[];
  editable: boolean;
  onCreate(): void;
  onOpen(role: CustomRole): void;
  onDelete(role: CustomRole): void;
}

export function RoleList({ roles, editable, onCreate, onOpen, onDelete }: RoleListProps) {
  return (
    <div className="settings-card roles-card">
      <div className="roles-card-header">
        <h3 className="roles-card-title">{LIST_TITLE}</h3>
        {editable && (
          <Button type="button" size="sm" onClick={onCreate}>
            {CREATE_LABEL}
          </Button>
        )}
      </div>
      {roles.length === 0 ? (
        <p className="roles-hint">{editable ? EMPTY_EDITABLE : EMPTY_READ_ONLY}</p>
      ) : (
        <ul className="roles-list">
          {roles.map((role) => (
            <li key={role.id} className="roles-item">
              <div className="roles-item-identity">
                <div className="roles-item-heading">
                  <span className="roles-item-name">{role.name}</span>
                  <Badge variant={role.scope === RoleScope.Workspace ? 'accent' : 'neutral'}>
                    {SCOPE_LABELS[role.scope]}
                  </Badge>
                </div>
                {role.description && (
                  <span className="roles-item-description">{role.description}</span>
                )}
                <span className="roles-item-meta">
                  {permissionCount(role.permissions.length)}
                  {META_SEPARATOR}
                  {memberCount(role.members)}
                </span>
              </div>
              <div className="roles-item-actions">
                {editable ? (
                  <>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      aria-label={`Edit ${role.name}`}
                      onClick={() => onOpen(role)}
                    >
                      Edit
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      aria-label={`Delete ${role.name}`}
                      onClick={() => onDelete(role)}
                    >
                      Delete
                    </Button>
                  </>
                ) : (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    aria-label={`View ${role.name}`}
                    onClick={() => onOpen(role)}
                  >
                    View
                  </Button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

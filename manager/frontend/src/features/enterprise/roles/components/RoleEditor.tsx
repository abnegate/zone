import { Button, Checkbox, Input, Modal } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import {
  changed,
  choices,
  createRequest,
  draftOf,
  emptyDraft,
  nameMissing,
  type RoleDraft,
  rescoped,
  toggled,
  updateRequest,
} from '../draft';
import { useCreateRole, useUpdateRole } from '../hooks';
import {
  BASE_HINT,
  CANCEL_LABEL,
  CREATE_LABEL,
  DESCRIPTION_HINT,
  DESCRIPTION_LABEL,
  NAME_LABEL,
  PERMISSIONS_LABEL,
  permissionDescription,
  permissionLabel,
  SAVE_LABEL,
  SCOPE_DESCRIPTIONS,
  SCOPE_LABEL,
  SCOPE_LABELS,
} from '../labels';
import { NAME_REQUIRED, RoleField, refusalField, refusalMessage } from '../messages';
import { type PermissionName, ROLE_BASE, ROLE_SCOPES, RoleCode, type RoleScope } from '../schemas';
import type { CustomRole, PermissionDefinition } from '../types';
import { RolesUpgradeNotice } from './RolesUpgradeNotice';

export const CREATE_TITLE = 'Create role';

export const DONE_LABEL = 'Done';

export const FIXED_SCOPE_HINT = "A role's scope can't change after it is created.";

interface RoleEditorProps {
  organizationId: string;
  catalogue: readonly PermissionDefinition[];
  role?: CustomRole;
  editable: boolean;
  onClose: () => void;
}

function title(role: CustomRole | undefined, editable: boolean): string {
  if (!role) {
    return CREATE_TITLE;
  }
  return editable ? `Edit ${role.name}` : role.name;
}

function elementId(base: string, permission: PermissionName): string {
  return `${base}-${permission.replace(/[^a-z_]/g, '-')}`;
}

export function RoleEditor({
  organizationId,
  catalogue,
  role,
  editable,
  onClose,
}: RoleEditorProps) {
  const base = useId();
  const creation = useCreateRole(organizationId);
  const update = useUpdateRole(organizationId);
  const mutation = role ? update : creation;
  const { pending, refusal, error } = mutation;
  const [draft, setDraft] = useState<RoleDraft>(() => (role ? draftOf(role) : emptyDraft()));
  const [attempted, setAttempted] = useState(false);

  const request = role ? updateRequest(role, draft, catalogue) : null;
  const unchanged = request !== null && !changed(request);
  const refused = refusal ? refusalMessage(refusal.code, refusal.members) : null;
  const refusedField = refusal ? refusalField(refusal.code) : null;
  const licenseRequired = refusal?.code === RoleCode.LicenseRequired;
  const formError = licenseRequired || refusedField !== null ? null : (refused ?? error);
  const nameError =
    refusedField === RoleField.Name
      ? (refused ?? undefined)
      : attempted && nameMissing(draft)
        ? NAME_REQUIRED
        : undefined;
  const permissionsError = refusedField === RoleField.Permissions ? refused : null;
  const disabled = !editable || pending;
  const roleBase = ROLE_BASE[draft.scope];

  const change = (next: RoleDraft) => {
    setDraft(next);
    if (error) {
      mutation.reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (disabled || unchanged) {
      return;
    }
    setAttempted(true);
    if (nameMissing(draft)) {
      return;
    }
    if (role && request) {
      update.update(role.id, request, onClose);
    } else {
      creation.create(createRequest(draft, catalogue), onClose);
    }
  };

  const scopeOption = (scope: RoleScope) => (
    <label key={scope} className="roles-scope-option">
      <input
        type="radio"
        name={`${base}-scope`}
        value={scope}
        checked={draft.scope === scope}
        disabled={disabled}
        onChange={() => change(rescoped(draft, scope, catalogue))}
      />
      <span className="roles-scope-copy">
        <span className="roles-scope-label">{SCOPE_LABELS[scope]}</span>
        <span className="roles-scope-description">{SCOPE_DESCRIPTIONS[scope]}</span>
      </span>
    </label>
  );

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={title(role, editable)} size="lg">
      <form className="ui-form roles-editor" onSubmit={submit} noValidate>
        <fieldset className="roles-fields" disabled={disabled}>
          <Input
            id={`${base}-name`}
            label={NAME_LABEL}
            value={draft.name}
            autoComplete="off"
            placeholder="Release manager"
            disabled={disabled}
            error={nameError}
            onChange={(event) => change({ ...draft, name: event.target.value })}
          />
          <Input
            id={`${base}-description`}
            label={DESCRIPTION_LABEL}
            value={draft.description}
            autoComplete="off"
            disabled={disabled}
            helpText={DESCRIPTION_HINT}
            onChange={(event) => change({ ...draft, description: event.target.value })}
          />
          {role ? (
            <div className="roles-scope-fixed">
              <span className="roles-group-title">{SCOPE_LABEL}</span>
              <span className="roles-scope-label">{SCOPE_LABELS[role.scope]}</span>
              <span className="roles-scope-description">{FIXED_SCOPE_HINT}</span>
            </div>
          ) : (
            <fieldset className="roles-group">
              <legend className="roles-group-title">{SCOPE_LABEL}</legend>
              <div className="roles-scopes">{ROLE_SCOPES.map(scopeOption)}</div>
            </fieldset>
          )}
          <fieldset className="roles-group">
            <legend className="roles-group-title">{PERMISSIONS_LABEL}</legend>
            <div className="roles-permissions">
              <div className="roles-permission">
                <Checkbox
                  id={elementId(base, roleBase)}
                  label={permissionLabel(roleBase)}
                  helpText={BASE_HINT}
                  checked
                  disabled
                />
              </div>
              {choices(catalogue, draft.scope).map((entry) => (
                <div key={entry.name} className="roles-permission">
                  <Checkbox
                    id={elementId(base, entry.name)}
                    label={permissionLabel(entry.name)}
                    helpText={permissionDescription(entry.name)}
                    checked={draft.permissions.includes(entry.name)}
                    disabled={disabled}
                    onCheckedChange={(on) =>
                      change({ ...draft, permissions: toggled(draft.permissions, entry.name, on) })
                    }
                  />
                </div>
              ))}
            </div>
            {permissionsError && (
              <p className="roles-field-error" role="alert">
                {permissionsError}
              </p>
            )}
          </fieldset>
        </fieldset>
        {licenseRequired && <RolesUpgradeNotice />}
        {formError && (
          <div className="modal-error" role="alert">
            {formError}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {editable ? CANCEL_LABEL : DONE_LABEL}
          </Button>
          {editable && (
            <Button type="submit" loading={pending} disabled={unchanged}>
              {role ? SAVE_LABEL : CREATE_LABEL}
            </Button>
          )}
        </div>
      </form>
    </Modal>
  );
}

import { Button, Modal } from '@zone/ui';
import { type FormEvent, useState } from 'react';
import {
  changed,
  createRequest,
  type DestinationDraft,
  type DraftErrors,
  draftErrors,
  draftOf,
  emptyDraft,
  updateRequest,
} from '../draft';
import { useCreateAuditDestination, useUpdateAuditDestination } from '../hooks';
import {
  ADD_DESTINATION_LABEL,
  CANCEL_LABEL,
  EDIT_DESTINATION_TITLE,
  NEW_DESTINATION_TITLE,
  SAVE_DESTINATION_LABEL,
} from '../labels';
import { refusalField, refusalMessage } from '../messages';
import { AuditCode } from '../schemas';
import type { AuditScope, DestinationResponse, SecretName } from '../types';
import { AuditUpgradeNotice } from './AuditUpgradeNotice';
import { DestinationFields, type FieldError } from './DestinationFields';

const NO_ERRORS: DraftErrors = {};

const NO_SECRETS: readonly SecretName[] = [];

interface DestinationFormProps {
  scope: AuditScope;
  destination?: DestinationResponse;
  onClose(): void;
}

export function DestinationForm({ scope, destination, onClose }: DestinationFormProps) {
  const creation = useCreateAuditDestination(scope);
  const update = useUpdateAuditDestination(scope);
  const [draft, setDraft] = useState<DestinationDraft>(() =>
    destination ? draftOf(destination) : emptyDraft()
  );
  const [attempted, setAttempted] = useState(false);

  const mutation = destination ? update : creation;
  const { pending, refusal, error } = mutation;
  const errors = attempted ? draftErrors(draft, destination) : NO_ERRORS;
  const unchanged = destination !== undefined && !changed(updateRequest(destination, draft));
  const refusedField = refusal ? refusalField(refusal.code, draft.kind) : null;
  const licenseRequired = refusal?.code === AuditCode.LicenseRequired;
  const formError =
    licenseRequired || refusedField !== null
      ? null
      : refusal
        ? refusalMessage(refusal.code)
        : error;

  const fieldError: FieldError = (field) => {
    if (refusal && refusedField === field) {
      return refusalMessage(refusal.code);
    }
    return errors[field];
  };

  const change = (next: DestinationDraft) => {
    setDraft(next);
    if (error) {
      mutation.reset();
    }
  };

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (pending || unchanged) {
      return;
    }
    setAttempted(true);
    if (Object.keys(draftErrors(draft, destination)).length > 0) {
      return;
    }
    if (destination) {
      update.update(destination.id, updateRequest(destination, draft), onClose);
    } else {
      creation.create(createRequest(draft), onClose);
    }
  };

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={destination ? EDIT_DESTINATION_TITLE : NEW_DESTINATION_TITLE}
      size="md"
    >
      <form className="ui-form audit-form" onSubmit={submit} noValidate>
        <DestinationFields
          draft={draft}
          saved={destination?.secrets ?? NO_SECRETS}
          editing={destination !== undefined}
          disabled={pending}
          error={fieldError}
          onChange={change}
        />
        {licenseRequired && (
          <AuditUpgradeNotice message={refusalMessage(AuditCode.LicenseRequired)} />
        )}
        {formError && (
          <div className="modal-error" role="alert">
            {formError}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button type="submit" loading={pending} disabled={unchanged}>
            {destination ? SAVE_DESTINATION_LABEL : ADD_DESTINATION_LABEL}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

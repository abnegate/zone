import { Button, Input, Modal } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { useCreateScimToken } from '../hooks';
import {
  CANCEL_LABEL,
  CREATE_LABEL,
  CREATE_TITLE,
  NAME_HINT,
  NAME_LABEL,
  NAME_PLACEHOLDER,
  NAME_REQUIRED,
} from '../labels';
import { refusalMessage } from '../messages';
import { ScimCode } from '../schemas';
import type { IssuedResponse } from '../types';

interface CreateScimTokenDialogProps {
  organizationId: string;
  onIssued: (issued: IssuedResponse) => void;
  onClose: () => void;
}

export function CreateScimTokenDialog({
  organizationId,
  onIssued,
  onClose,
}: CreateScimTokenDialogProps) {
  const id = useId();
  const { create, pending, refusal, error, reset } = useCreateScimToken(organizationId);
  const [name, setName] = useState('');
  const [attempted, setAttempted] = useState(false);
  const blank = name.trim() === '';
  const nameRefused = refusal?.code === ScimCode.TokenNameInvalid;
  const nameError = nameRefused || (attempted && blank) ? NAME_REQUIRED : undefined;
  const formError = nameRefused ? null : refusal ? refusalMessage(refusal.code) : error;

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setAttempted(true);
    if (blank || pending) {
      return;
    }
    create(name.trim(), onIssued);
  };

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={CREATE_TITLE} size="sm">
      <form className="ui-form scim-dialog" onSubmit={submit} noValidate>
        <Input
          id={`${id}-name`}
          label={NAME_LABEL}
          value={name}
          autoComplete="off"
          placeholder={NAME_PLACEHOLDER}
          helpText={NAME_HINT}
          disabled={pending}
          error={nameError}
          onChange={(event) => {
            setName(event.target.value);
            if (error) {
              reset();
            }
          }}
        />
        {formError && (
          <div className="modal-error" role="alert">
            {formError}
          </div>
        )}
        <div className="modal-actions">
          <Button type="button" variant="ghost" disabled={pending} onClick={onClose}>
            {CANCEL_LABEL}
          </Button>
          <Button type="submit" loading={pending}>
            {CREATE_LABEL}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

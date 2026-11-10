import { Button, Input, Modal, Select, type SelectOption } from '@zone/ui';
import { type FormEvent, useId, useState } from 'react';
import { Transport } from './constants';
import {
  changed,
  createRequest,
  type DraftErrors,
  DraftField,
  draftErrors,
  draftOf,
  emptyDraft,
  type McpServerDraft,
  updateRequest,
  usesCommand,
} from './draft';
import { McpSecretRows } from './McpSecretRows';
import { type McpServerSubmission, SubmissionKind } from './submission';
import type { McpServer, McpTransport } from './types';
import './McpServerForm.css';

export const COMMANDS_OFF_NOTICE =
  'Command servers are turned off on this instance (ZONE_MCP_COMMANDS)';
export const ARGUMENTS_HINT = 'One argument per line';
export const SAVE_FAILED = 'Failed to save the MCP server';

const TRANSPORT_LABELS: Readonly<Record<McpTransport, string>> = {
  [Transport.Http]: 'Streamable HTTP',
  [Transport.Stdio]: 'Command (stdio)',
  [Transport.Sse]: 'SSE',
};

const NO_ERRORS: DraftErrors = {};

function transportOptions(commands: boolean, current: McpTransport): SelectOption[] {
  const transports: McpTransport[] = [Transport.Http];
  if (commands || current === Transport.Stdio) {
    transports.push(Transport.Stdio);
  }
  if (!transports.includes(current)) {
    transports.push(current);
  }
  return transports.map((value) => ({ value, label: TRANSPORT_LABELS[value] }));
}

function isTransport(value: string): value is McpTransport {
  return Object.values<string>(Transport).includes(value);
}

interface McpServerFormProps {
  server?: McpServer;
  commands: boolean;
  onSubmit: (submission: McpServerSubmission) => Promise<void>;
  onClose: () => void;
}

export function McpServerForm({ server, commands, onSubmit, onClose }: McpServerFormProps) {
  const base = useId();
  const argumentsId = `${base}-arguments`;
  const argumentsHintId = `${base}-arguments-hint`;
  const argumentsErrorId = `${base}-arguments-error`;
  const [draft, setDraft] = useState<McpServerDraft>(() =>
    server ? draftOf(server) : emptyDraft(Transport.Http)
  );
  const [attempted, setAttempted] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const editing = server !== undefined;
  const errors = attempted ? draftErrors(draft, editing) : NO_ERRORS;
  const blocked = server !== undefined && usesCommand(server.transport) && !commands;
  const unchanged = server !== undefined && !changed(updateRequest(server, draft));
  const command = usesCommand(draft.transport);
  const argumentsError = errors[DraftField.Arguments];

  const update = <K extends keyof McpServerDraft>(key: K, value: McpServerDraft[K]) =>
    setDraft((current) => ({ ...current, [key]: value }));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (pending || blocked || unchanged) {
      return;
    }
    setAttempted(true);
    if (Object.keys(draftErrors(draft, editing)).length > 0) {
      return;
    }
    const submission: McpServerSubmission = server
      ? { kind: SubmissionKind.Update, request: updateRequest(server, draft) }
      : { kind: SubmissionKind.Create, request: createRequest(draft) };
    setPending(true);
    setError(null);
    try {
      await onSubmit(submission);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : SAVE_FAILED);
    } finally {
      setPending(false);
    }
  };

  return (
    <Modal
      isOpen
      onClose={pending ? undefined : onClose}
      title={server ? `Edit ${server.name}` : 'Add MCP server'}
      size="md"
    >
      <form className="ui-form mcp-server-form" onSubmit={submit} noValidate>
        {blocked && (
          <p className="form-hint mcp-server-notice" role="status">
            {COMMANDS_OFF_NOTICE}
          </p>
        )}
        <fieldset className="mcp-server-fields" disabled={pending}>
          <Select
            id={`${base}-transport`}
            label="Transport"
            options={transportOptions(commands, draft.transport)}
            value={draft.transport}
            disabled={editing || pending}
            onValueChange={(value) => {
              if (isTransport(value)) {
                update('transport', value);
              }
            }}
          />
          <Input
            id={`${base}-name`}
            label="Name"
            value={draft.name}
            autoComplete="off"
            spellCheck={false}
            error={errors[DraftField.Name]}
            onChange={(event) => update('name', event.target.value)}
          />
          {command ? (
            <>
              <Input
                id={`${base}-command`}
                label="Command"
                value={draft.command}
                autoComplete="off"
                spellCheck={false}
                error={errors[DraftField.Command]}
                onChange={(event) => update('command', event.target.value)}
              />
              <div className="form-group">
                <label htmlFor={argumentsId}>Arguments</label>
                <textarea
                  id={argumentsId}
                  className="mcp-server-arguments"
                  rows={3}
                  value={draft.arguments}
                  spellCheck={false}
                  aria-invalid={argumentsError !== undefined}
                  aria-describedby={argumentsError ? argumentsErrorId : argumentsHintId}
                  onChange={(event) => update('arguments', event.target.value)}
                />
                {argumentsError ? (
                  <p id={argumentsErrorId} className="field-error" role="alert">
                    {argumentsError}
                  </p>
                ) : (
                  <p id={argumentsHintId} className="form-hint">
                    {ARGUMENTS_HINT}
                  </p>
                )}
              </div>
              <McpSecretRows
                list={DraftField.Environment}
                rows={draft.environment}
                errors={errors}
                onChange={(rows) => update('environment', rows)}
              />
            </>
          ) : (
            <>
              <Input
                id={`${base}-url`}
                label="URL"
                type="url"
                inputMode="url"
                value={draft.url}
                autoComplete="off"
                spellCheck={false}
                placeholder="https://example.com/mcp"
                error={errors[DraftField.Url]}
                onChange={(event) => update('url', event.target.value)}
              />
              <McpSecretRows
                list={DraftField.Headers}
                rows={draft.headers}
                errors={errors}
                onChange={(rows) => update('headers', rows)}
              />
            </>
          )}
        </fieldset>
        {error && (
          <div className="modal-error" role="alert">
            {error}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" loading={pending} disabled={blocked || unchanged}>
            {server ? 'Save' : 'Add server'}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

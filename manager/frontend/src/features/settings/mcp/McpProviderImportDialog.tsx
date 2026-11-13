import { Badge, Button, Checkbox, EmptyState, Modal } from '@zone/ui';
import { useId, useMemo, useState } from 'react';
import { Status } from './constants';
import { messageOf } from './failure';
import { type McpProviderSelection, useProviderMcpServers } from './hooks';
import { importLabel } from './McpImportDialog';
import { STATUS_BADGES } from './status';
import { TRANSPORT_LABELS } from './transport';
import type { ProviderMcpServer, ProviderMcpServers } from './types';
import './McpProviderImportDialog.css';

export const TITLE = 'Import from a provider';
export const IMPORT_HINT =
  'The servers your connected coding agents have configured. Zone does not attach them to a turn itself, so importing one is what makes its tools available.';
export const IMPORT_FAILED = 'Failed to import the MCP servers';
export const READING = 'Reading what your providers have configured…';
export const NO_PROVIDERS = 'No connected provider';
export const NO_PROVIDERS_HINT =
  'Zone holds no coding agent sign-in for this organization, so there is nothing to read.';
export const NOTHING_CONFIGURED = 'configures no MCP servers';
export const NOT_IMPORTED = 'Not imported';
export const SKIPPED = 'Skipped';

const DISABLED = STATUS_BADGES[Status.Disabled];

export function ignoredNote(keys: readonly string[]): string {
  return `${NOT_IMPORTED}: ${keys.join(', ')}`;
}

/** The key one server of one provider is chosen by; a name is unique within a provider. */
export function keyOf(provider: string, name: string): string {
  return `${provider}/${name}`;
}

function importable(server: ProviderMcpServer): boolean {
  return server.refusal === null;
}

function target(server: ProviderMcpServer): string {
  return server.url ?? [server.command, ...server.arguments].filter(Boolean).join(' ');
}

interface McpProviderImportDialogProps {
  organizationId: string;
  onImport: (selections: McpProviderSelection[]) => Promise<void>;
  onClose: () => void;
}

export function McpProviderImportDialog({
  organizationId,
  onImport,
  onClose,
}: McpProviderImportDialogProps) {
  const base = useId();
  const hintId = `${base}-hint`;
  const { providers, loading, loaded, error } = useProviderMcpServers(organizationId);
  const [chosen, setChosen] = useState<ReadonlySet<string> | null>(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  const offered = useMemo(
    () =>
      new Set(
        providers.flatMap((provider) =>
          provider.servers.filter(importable).map((server) => keyOf(provider.provider, server.name))
        )
      ),
    [providers]
  );
  const selected = chosen ?? offered;

  const selections: McpProviderSelection[] = providers
    .map((provider) => ({
      provider: provider.provider,
      label: provider.label,
      servers: provider.servers
        .filter(
          (server) => importable(server) && selected.has(keyOf(provider.provider, server.name))
        )
        .map((server) => server.name),
    }))
    .filter((selection) => selection.servers.length > 0);
  const count = selections.reduce((total, selection) => total + selection.servers.length, 0);

  const toggle = (key: string, checked: boolean) =>
    setChosen((current) => {
      const next = new Set(current ?? offered);
      if (checked) {
        next.add(key);
      } else {
        next.delete(key);
      }
      return next;
    });

  const submit = async () => {
    if (pending || count === 0) {
      return;
    }
    setPending(true);
    setFailure(null);
    try {
      await onImport(selections);
    } catch (refused) {
      setFailure(messageOf(refused, IMPORT_FAILED));
    } finally {
      setPending(false);
    }
  };

  return (
    <Modal isOpen onClose={pending ? undefined : onClose} title={TITLE} size="md">
      <div className="mcp-provider-import">
        <p id={hintId} className="form-hint">
          {IMPORT_HINT}
        </p>
        {loading && (
          <div className="loading-state" role="status">
            {READING}
          </div>
        )}
        {error !== null && (
          <div className="alert alert-error" role="alert">
            {error}
          </div>
        )}
        {loaded && providers.length === 0 && (
          <EmptyState title={NO_PROVIDERS} description={NO_PROVIDERS_HINT} />
        )}
        {providers.map((provider) => (
          <ProviderGroup
            key={provider.provider}
            base={base}
            provider={provider}
            selected={selected}
            pending={pending}
            onToggle={toggle}
          />
        ))}
        {failure !== null && (
          <div className="modal-error" role="alert">
            {failure}
          </div>
        )}
        <div className="modal-actions">
          <Button variant="ghost" disabled={pending} onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={count === 0} loading={pending} onClick={submit}>
            {importLabel(count)}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

interface ProviderGroupProps {
  base: string;
  provider: ProviderMcpServers;
  selected: ReadonlySet<string>;
  pending: boolean;
  onToggle: (key: string, checked: boolean) => void;
}

function ProviderGroup({ base, provider, selected, pending, onToggle }: ProviderGroupProps) {
  const headingId = `${base}-${provider.provider}`;
  const skippedId = `${headingId}-skipped`;

  return (
    <div className="mcp-provider-import-group">
      <p id={headingId} className="form-label">
        {provider.label}
      </p>
      {provider.servers.length === 0 && (
        <p className="form-hint">{`${provider.label} ${NOTHING_CONFIGURED}.`}</p>
      )}
      {provider.servers.length > 0 && (
        <ul className="mcp-provider-import-list" aria-labelledby={headingId}>
          {provider.servers.map((server) => {
            const key = keyOf(provider.provider, server.name);
            return (
              <li key={key} className="mcp-provider-import-server" aria-label={server.name}>
                <Checkbox
                  id={`${base}-${key}`}
                  label={server.name}
                  checked={selected.has(key)}
                  disabled={pending || !importable(server)}
                  onCheckedChange={(checked) => onToggle(key, checked)}
                />
                <div className="mcp-provider-import-detail">
                  <Badge variant="outline">{TRANSPORT_LABELS[server.transport]}</Badge>
                  {!server.enabled && <Badge variant={DISABLED.variant}>{DISABLED.label}</Badge>}
                  <span className="mcp-provider-import-target">{target(server)}</span>
                </div>
                {server.ignored.length > 0 && (
                  <span className="mcp-provider-import-note">{ignoredNote(server.ignored)}</span>
                )}
                {server.refusal !== null && (
                  <span className="mcp-provider-import-refusal">{server.refusal}</span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {provider.errors.length > 0 && (
        <>
          <p id={skippedId} className="form-label">
            {SKIPPED}
          </p>
          <ul className="mcp-provider-import-list" aria-labelledby={skippedId}>
            {provider.errors.map((message) => (
              <li key={message} className="mcp-provider-import-note">
                {message}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

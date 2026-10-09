import { Button } from '@zone/ui';
import { useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import type { HostDirectories, HostMounts } from '../types';
import './HostFoldersPanel.css';

type FolderRow = { id: string; path: string };

function emptyMounts(): HostMounts {
  return {
    in_container: false,
    host_root: null,
    container_root: null,
    ready: true,
    hint: '',
  };
}

function folderRow(path: string): FolderRow {
  return { id: crypto.randomUUID(), path };
}

function rowsFrom(directories: string[]): FolderRow[] {
  return directories.length ? directories.map(folderRow) : [folderRow('')];
}

export default function HostFoldersPanel({
  workspaceId,
  orgId,
  variant = 'settings',
  onSaved,
  onSkip,
}: {
  workspaceId: string | null;
  orgId: string | null;
  variant?: 'settings' | 'setup';
  onSaved?: () => void;
  onSkip?: () => void;
}) {
  const [mounts, setMounts] = useState<HostMounts | null>(null);
  const [rows, setRows] = useState<FolderRow[]>(() => [folderRow('')]);
  const [folders, setFolders] = useState<HostDirectories['folders']>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!workspaceId || !orgId) {
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const [nextMounts, nextDirectories] = await Promise.all([
        client.getHostMounts(),
        client.getWorkspaceHostDirectories(orgId, workspaceId),
      ]);
      setMounts(nextMounts);
      setFolders(nextDirectories.folders);
      setRows(rowsFrom(nextDirectories.directories));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to load host folders');
      setMounts(emptyMounts());
    } finally {
      setLoading(false);
    }
  }, [orgId, workspaceId]);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!workspaceId || !orgId) return;
    setSaving(true);
    setError(null);
    setSuccess(null);
    try {
      const saved = await client.updateWorkspaceHostDirectories(orgId, workspaceId, {
        directories: rows.map((row) => row.path.trim()).filter(Boolean),
      });
      setFolders(saved.folders);
      setRows(rowsFrom(saved.directories));
      setSuccess('Folders saved');
      onSaved?.();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Failed to save host folders');
    } finally {
      setSaving(false);
    }
  };

  const info = mounts ?? emptyMounts();
  const placeholder = info.host_root
    ? `${info.host_root}/my-project`
    : '/Users/you/Local/my-project';

  if (loading) {
    return <div className="host-folders-status">Loading host folders...</div>;
  }

  if (!workspaceId || !orgId) {
    return (
      <div className="host-folders">
        <p className="host-folders-hint">Select a workspace before choosing host folders.</p>
        {onSkip && (
          <Button variant="link" onClick={onSkip}>
            Skip for now
          </Button>
        )}
      </div>
    );
  }

  return (
    <div className={variant === 'setup' ? 'host-folders host-folders--setup' : 'host-folders'}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Host folders</h2>
          <p className="section-description">
            Directories on this machine chat tools can read and write. Pick folders under the host
            root; the first one that exists becomes the chat working directory.
          </p>
        </div>
      </div>

      {info.hint && (
        <p className={`host-folders-hint${info.ready ? '' : ' host-folders-hint--warn'}`}>
          {info.hint}
        </p>
      )}

      {error && <div className="alert alert-error">{error}</div>}
      {success && variant === 'settings' && <div className="alert alert-success">{success}</div>}

      <ul className="host-folders-list">
        {rows.map((row, index) => {
          const mapped = folders.find((folder) => folder.host === row.path.trim());
          return (
            <li key={row.id} className="host-folders-row">
              <label className="host-folders-field">
                <span className="host-folders-label">Folder {index + 1}</span>
                <input
                  type="text"
                  value={row.path}
                  placeholder={placeholder}
                  spellCheck={false}
                  autoComplete="off"
                  onChange={(event) => {
                    const path = event.target.value;
                    setRows((current) =>
                      current.map((item) => (item.id === row.id ? { ...item, path } : item))
                    );
                  }}
                />
              </label>
              {mapped?.mapped && (
                <p className="host-folders-mapped">
                  {mapped.exists ? 'Visible as' : 'Not visible yet:'} <code>{mapped.mapped}</code>
                </p>
              )}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  setRows((current) => {
                    const next = current.filter((item) => item.id !== row.id);
                    return next.length ? next : [folderRow('')];
                  });
                }}
              >
                Remove
              </Button>
            </li>
          );
        })}
      </ul>

      <div className="host-folders-toolbar">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={() => setRows((current) => [...current, folderRow('')])}
        >
          Add folder
        </Button>
      </div>

      <div className="host-folders-actions">
        {onSkip && (
          <Button type="button" variant="link" onClick={onSkip} disabled={saving}>
            Skip for now
          </Button>
        )}
        <Button type="button" variant="primary" onClick={() => void save()} loading={saving}>
          {saving ? 'Saving...' : variant === 'setup' ? 'Save and continue' : 'Save Changes'}
        </Button>
      </div>
    </div>
  );
}

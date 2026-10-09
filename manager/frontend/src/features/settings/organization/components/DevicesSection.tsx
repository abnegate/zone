import { Button, Checkbox, EmptyState, Input, Modal } from '@zone/ui';
import { useCallback, useEffect, useState } from 'react';
import { client } from '../../../../api/client';
import type { DevicePolicyMode, OrganizationDevice } from '../types';
import './DevicesSection.css';

interface DevicesSectionProps {
  orgId: string;
}

const PLATFORM_LABEL: Record<OrganizationDevice['platform'], string> = {
  android: 'Android',
  ios: 'iOS',
  desktop: 'Desktop',
  browser: 'Browser',
  cli: 'CLI',
};

const toUserMessage = (err: unknown, fallback: string) => {
  const message = err instanceof Error ? err.message : fallback;
  return message.startsWith('Validation failed') ? fallback : message;
};

const deviceLabel = (device: OrganizationDevice): string =>
  device.name?.trim() || PLATFORM_LABEL[device.platform];

const memberLabel = (device: OrganizationDevice): string =>
  device.display_name?.trim() || device.email;

export default function DevicesSection({ orgId }: DevicesSectionProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | undefined>(undefined);
  const [success, setSuccess] = useState<string | undefined>(undefined);
  const [devices, setDevices] = useState<OrganizationDevice[]>([]);
  const [mode, setMode] = useState<DevicePolicyMode>('open');
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [savingPolicy, setSavingPolicy] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState('');
  const [blockTarget, setBlockTarget] = useState<OrganizationDevice | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(undefined);
    try {
      const [listed, policy] = await Promise.all([
        client.getDevices(orgId),
        client.getDevicePolicy(orgId),
      ]);
      setDevices(listed.devices);
      setMode(policy.mode);
    } catch (err) {
      setError(toUserMessage(err, 'Couldn’t load devices'));
    } finally {
      setLoading(false);
    }
  }, [orgId]);

  useEffect(() => {
    load();
  }, [load]);

  const flash = (message: string) => {
    setSuccess(message);
    setTimeout(() => setSuccess(undefined), 3000);
  };

  const setStatus = async (device: OrganizationDevice, status: 'allowed' | 'blocked') => {
    setUpdatingId(device.id);
    setError(undefined);
    try {
      const updated = await client.updateDevice(orgId, device.id, { status });
      setDevices((current) => current.map((row) => (row.id === updated.id ? updated : row)));
      flash(status === 'allowed' ? 'Device allowed' : 'Device blocked');
    } catch (err) {
      setError(toUserMessage(err, 'Couldn’t update device'));
    } finally {
      setUpdatingId(null);
      setBlockTarget(null);
    }
  };

  const saveName = async (device: OrganizationDevice) => {
    const name = editingName.trim();
    setEditingId(null);
    if (name === (device.name ?? '')) {
      return;
    }
    setUpdatingId(device.id);
    setError(undefined);
    try {
      const updated = await client.updateDevice(orgId, device.id, { name });
      setDevices((current) => current.map((row) => (row.id === updated.id ? updated : row)));
      flash('Device renamed');
    } catch (err) {
      setError(toUserMessage(err, 'Couldn’t rename device'));
    } finally {
      setUpdatingId(null);
    }
  };

  const setPolicy = async (next: boolean) => {
    const nextMode: DevicePolicyMode = next ? 'allowed' : 'open';
    setSavingPolicy(true);
    setError(undefined);
    try {
      const policy = await client.setDevicePolicy(orgId, nextMode);
      setMode(policy.mode);
      flash(
        policy.mode === 'allowed'
          ? 'Only allowed devices may connect'
          : 'Any signed-in device may connect'
      );
    } catch (err) {
      setError(toUserMessage(err, 'Couldn’t update device policy'));
    } finally {
      setSavingPolicy(false);
    }
  };

  if (loading) {
    return <div className="loading-state">Loading devices...</div>;
  }

  return (
    <div className="org-devices-section">
      <div className="section-row">
        <div className="section-row-copy">
          <h2 className="section-title">Devices</h2>
          <p className="section-description">
            See which clients are signed in, and block ones that should not reach this instance.
          </p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error" role="alert">
          {error}
        </div>
      )}
      {success && (
        <div className="alert alert-success" role="alert">
          {success}
        </div>
      )}
      <div className="settings-card">
        <Checkbox
          label="Only allowed devices may connect"
          helpText="Unknown clients wait for an admin. Blocking a device signs it out and keeps it out."
          checked={mode === 'allowed'}
          disabled={savingPolicy}
          onCheckedChange={setPolicy}
        />
      </div>
      {devices.length === 0 ? (
        <EmptyState title="No devices yet" />
      ) : (
        <div className="devices-table-container">
          <table className="devices-table">
            <thead>
              <tr>
                <th>Device</th>
                <th>Member</th>
                <th>Status</th>
                <th>Last seen</th>
                <th className="devices-actions-head">Actions</th>
              </tr>
            </thead>
            <tbody>
              {devices.map((device) => {
                const busy = updatingId === device.id;
                const editing = editingId === device.id;
                return (
                  <tr key={device.id}>
                    <td>
                      <div className="device-identity">
                        {editing ? (
                          <Input
                            aria-label="Device name"
                            value={editingName}
                            autoFocus
                            disabled={busy}
                            onChange={(event) => setEditingName(event.target.value)}
                            onBlur={() => saveName(device)}
                            onKeyDown={(event) => {
                              if (event.key === 'Enter') {
                                event.currentTarget.blur();
                              }
                              if (event.key === 'Escape') {
                                setEditingId(null);
                              }
                            }}
                          />
                        ) : (
                          <button
                            type="button"
                            className="device-name"
                            onClick={() => {
                              setEditingId(device.id);
                              setEditingName(device.name ?? '');
                            }}
                          >
                            {deviceLabel(device)}
                          </button>
                        )}
                        <div className="device-meta">
                          {PLATFORM_LABEL[device.platform]}
                          {device.last_ip ? ` · ${device.last_ip}` : ''}
                        </div>
                      </div>
                    </td>
                    <td>
                      <div className="device-member">{memberLabel(device)}</div>
                      {device.display_name && <div className="device-email">{device.email}</div>}
                    </td>
                    <td>
                      <span className={`device-status device-status-${device.status}`}>
                        {device.status}
                      </span>
                      {device.connected && <span className="device-connected">Connected</span>}
                    </td>
                    <td className="device-seen">
                      {new Date(device.last_seen_at).toLocaleString()}
                    </td>
                    <td>
                      <div className="device-actions">
                        {device.status !== 'allowed' && (
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={busy}
                            aria-label={`Allow ${deviceLabel(device)}`}
                            onClick={() => setStatus(device, 'allowed')}
                          >
                            Allow
                          </Button>
                        )}
                        {device.status !== 'blocked' && (
                          <Button
                            size="sm"
                            variant="ghost"
                            className="device-block"
                            disabled={busy}
                            aria-label={`Block ${deviceLabel(device)}`}
                            onClick={() => setBlockTarget(device)}
                          >
                            Block
                          </Button>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <Modal
        isOpen={blockTarget !== null}
        onClose={() => setBlockTarget(null)}
        title="Block this device?"
        size="sm"
      >
        {blockTarget && (
          <div className="confirm-modal">
            <p>
              {deviceLabel(blockTarget)} for {memberLabel(blockTarget)} will be signed out and
              cannot sign in again until you allow it.
            </p>
            <div className="modal-actions">
              <Button variant="ghost" onClick={() => setBlockTarget(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                loading={updatingId === blockTarget.id}
                onClick={() => setStatus(blockTarget, 'blocked')}
              >
                Block
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}

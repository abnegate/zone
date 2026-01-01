import React from 'react';
import { Checkbox, Select, Input } from '../components';
import type { InstallerConfig } from '../types';

interface VPNStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
}

const providerOptions = [
  { value: 'surfshark', label: 'Surfshark' },
  { value: 'nordvpn', label: 'NordVPN' },
  { value: 'expressvpn', label: 'ExpressVPN' },
  { value: 'protonvpn', label: 'ProtonVPN' },
  { value: 'mullvad', label: 'Mullvad' },
];

const protocolOptions = [
  { value: 'openvpn', label: 'OpenVPN' },
  { value: 'wireguard', label: 'WireGuard' },
];

export function VPNStep({ config, onChange }: VPNStepProps) {
  const vpnEnabled = config.ENABLE_VPN === 'true';
  const isWireGuard = config.VPN_PROTOCOL === 'wireguard';

  return (
    <div className="step-content">
      <h2>VPN Configuration</h2>
      <p>Optional: Configure VPN for private web search</p>

      <div className="form-field">
        <Checkbox
          label="Enable VPN-protected search"
          checked={vpnEnabled}
          onChange={e => onChange('ENABLE_VPN', e.target.checked ? 'true' : 'false')}
        />
      </div>

      {vpnEnabled && (
        <div className="conditional-fields">
          <Select
            label="VPN Provider"
            options={providerOptions}
            value={config.VPN_PROVIDER}
            onChange={e => onChange('VPN_PROVIDER', e.target.value)}
          />

          <Select
            label="Protocol"
            options={protocolOptions}
            value={config.VPN_PROTOCOL}
            onChange={e => onChange('VPN_PROTOCOL', e.target.value)}
          />

          {!isWireGuard ? (
            <>
              <Input
                label="Username"
                type="text"
                value={config.OPENVPN_USER}
                onChange={e => onChange('OPENVPN_USER', e.target.value)}
              />
              <Input
                label="Password"
                type="password"
                value={config.OPENVPN_PASS}
                onChange={e => onChange('OPENVPN_PASS', e.target.value)}
              />
            </>
          ) : (
            <>
              <Input
                label="Private Key"
                type="text"
                value={config.WIREGUARD_PRIVATE_KEY}
                onChange={e => onChange('WIREGUARD_PRIVATE_KEY', e.target.value)}
                className="font-mono"
              />
              <Input
                label="Address"
                type="text"
                value={config.WIREGUARD_ADDRESS}
                onChange={e => onChange('WIREGUARD_ADDRESS', e.target.value)}
                placeholder="10.x.x.x/32"
                className="font-mono"
              />
            </>
          )}
        </div>
      )}
    </div>
  );
}

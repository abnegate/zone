import type React from 'react';
import { InfoBox, Input, Select } from '../components';
import type { InstallerConfig } from '../types';

interface VPNStepProps {
  config: InstallerConfig;
  onChange: (key: keyof InstallerConfig, value: string) => void;
  getFieldError: (field: string) => string | undefined;
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

export function VPNStep({ config, onChange, getFieldError }: VPNStepProps) {
  const isWireGuard = config.VPN_TYPE === 'wireguard';

  return (
    <div className="step-content">
      <div className="step-header">
        <h2>VPN Configuration</h2>
        <p>Optional: Configure VPN for private web search</p>
      </div>

      <Select
        label="VPN Provider"
        options={providerOptions}
        value={config.VPN_SERVICE_PROVIDER}
        onChange={(e: React.ChangeEvent<HTMLSelectElement>) =>
          onChange('VPN_SERVICE_PROVIDER', e.target.value)
        }
      />

      <Select
        label="Protocol"
        options={protocolOptions}
        value={config.VPN_TYPE}
        onChange={(e: React.ChangeEvent<HTMLSelectElement>) => onChange('VPN_TYPE', e.target.value)}
      />

      {!isWireGuard ? (
        <>
          <Input
            label="Username"
            type="text"
            value={config.VPN_OPENVPN_USER}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('VPN_OPENVPN_USER', e.target.value)
            }
            error={getFieldError('VPN_OPENVPN_USER')}
          />
          <Input
            label="Password"
            type="password"
            value={config.VPN_OPENVPN_PASSWORD}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('VPN_OPENVPN_PASSWORD', e.target.value)
            }
            error={getFieldError('VPN_OPENVPN_PASSWORD')}
          />
        </>
      ) : (
        <>
          <Input
            label="Private Key"
            type="text"
            value={config.VPN_WIREGUARD_PRIVATE_KEY}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('VPN_WIREGUARD_PRIVATE_KEY', e.target.value)
            }
            className="font-mono"
            error={getFieldError('VPN_WIREGUARD_PRIVATE_KEY')}
          />
          <Input
            label="Addresses"
            type="text"
            value={config.VPN_WIREGUARD_ADDRESSES}
            onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
              onChange('VPN_WIREGUARD_ADDRESSES', e.target.value)
            }
            placeholder="10.x.x.x/32"
            className="font-mono"
            error={getFieldError('VPN_WIREGUARD_ADDRESSES')}
          />
        </>
      )}

      <h3 className="section-header">Server Location (Optional)</h3>

      <Input
        label="Country"
        type="text"
        value={config.VPN_SERVER_COUNTRIES}
        onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
          onChange('VPN_SERVER_COUNTRIES', e.target.value)
        }
        placeholder="United States"
        helpText="e.g., United States, Germany, Japan"
        error={getFieldError('VPN_SERVER_COUNTRIES')}
      />

      <Input
        label="City"
        type="text"
        value={config.VPN_SERVER_CITIES}
        onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
          onChange('VPN_SERVER_CITIES', e.target.value)
        }
        placeholder="New York"
        helpText="e.g., New York, Los Angeles, London"
        error={getFieldError('VPN_SERVER_CITIES')}
      />

      <Input
        label="Region"
        type="text"
        value={config.VPN_SERVER_REGIONS}
        onChange={(e: React.ChangeEvent<HTMLInputElement>) =>
          onChange('VPN_SERVER_REGIONS', e.target.value)
        }
        placeholder="California"
        helpText="e.g., California, Texas"
        error={getFieldError('VPN_SERVER_REGIONS')}
      />

      <InfoBox variant="info">
        VPN is optional. Start with{' '}
        <code
          style={{
            background: 'var(--bg-base)',
            padding: '0.25rem 0.5rem',
            borderRadius: '0.25rem',
          }}
        >
          docker compose --profile vpn up
        </code>{' '}
        to enable.
      </InfoBox>
    </div>
  );
}

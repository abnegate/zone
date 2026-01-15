import { Controller, useFormContext } from 'react-hook-form';
import { AlertDescription, InfoBox, Input, SectionHeader, Select } from '../components';
import type { InstallerConfig } from '../types';

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

export function VPNStep() {
  const {
    register,
    watch,
    control,
    formState: { errors },
  } = useFormContext<InstallerConfig>();
  const isWireGuard = watch('VPN_TYPE') === 'wireguard';

  return (
    <div className="space-y-6">
      <div className="space-y-4">
        <Controller
          control={control}
          name="VPN_SERVICE_PROVIDER"
          render={({ field }) => (
            <Select
              label="VPN Provider"
              options={providerOptions}
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />

        <Controller
          control={control}
          name="VPN_TYPE"
          render={({ field }) => (
            <Select
              label="Protocol"
              options={protocolOptions}
              value={field.value}
              onValueChange={field.onChange}
              name={field.name}
            />
          )}
        />

        {!isWireGuard ? (
          <>
            <Input
              label="Username"
              type="text"
              error={errors.VPN_OPENVPN_USER?.message}
              {...register('VPN_OPENVPN_USER')}
            />
            <Input
              label="Password"
              type="password"
              error={errors.VPN_OPENVPN_PASSWORD?.message}
              {...register('VPN_OPENVPN_PASSWORD')}
            />
          </>
        ) : (
          <>
            <Input
              label="Private Key"
              type="text"
              className="font-mono"
              error={errors.VPN_WIREGUARD_PRIVATE_KEY?.message}
              {...register('VPN_WIREGUARD_PRIVATE_KEY')}
            />
            <Input
              label="Addresses"
              type="text"
              placeholder="10.x.x.x/32"
              className="font-mono"
              error={errors.VPN_WIREGUARD_ADDRESSES?.message}
              {...register('VPN_WIREGUARD_ADDRESSES')}
            />
          </>
        )}
      </div>

      <div className="space-y-4">
        <SectionHeader title="Server Location (Optional)" />
        <Input
          label="Country"
          type="text"
          placeholder="United States"
          helpText="e.g., United States, Germany, Japan"
          error={errors.VPN_SERVER_COUNTRIES?.message}
          {...register('VPN_SERVER_COUNTRIES')}
        />

        <Input
          label="City"
          type="text"
          placeholder="New York"
          helpText="e.g., New York, Los Angeles, London"
          error={errors.VPN_SERVER_CITIES?.message}
          {...register('VPN_SERVER_CITIES')}
        />

        <Input
          label="Region"
          type="text"
          placeholder="California"
          helpText="e.g., California, Texas"
          error={errors.VPN_SERVER_REGIONS?.message}
          {...register('VPN_SERVER_REGIONS')}
        />
      </div>

      <InfoBox variant="info">
        <AlertDescription className="flex flex-wrap items-center gap-2">
          <span>VPN is optional. Start with</span>
          <code className="rounded-md bg-muted px-2 py-1 text-xs">
            docker compose --profile vpn up
          </code>
          <span>to enable.</span>
        </AlertDescription>
      </InfoBox>
    </div>
  );
}

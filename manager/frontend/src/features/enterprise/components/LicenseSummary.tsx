import { Badge, type BadgeProps, Button } from '@zone/ui';
import {
  editionLabel,
  expiryLabel,
  licenseDate,
  seatText,
  statusLabel,
  statusTone,
  storesLicense,
  Tone,
  unverifiedReason,
} from '../status';
import type { LicenseResponse } from '../types';

const BADGE_VARIANTS: Readonly<Record<Tone, BadgeProps['variant']>> = {
  [Tone.Neutral]: 'neutral',
  [Tone.Success]: 'success',
  [Tone.Warning]: 'warning',
  [Tone.Error]: 'destructive',
};

interface LicenseSummaryProps {
  response: LicenseResponse;
  changing: boolean;
  onRemove: () => void;
}

export function LicenseSummary({ response, changing, onRemove }: LicenseSummaryProps) {
  const { edition, status, license, seats, unverified } = response;

  return (
    <div className="settings-card license-summary">
      <div className="license-summary-header">
        <div className="license-edition">
          <span className="license-edition-label">Edition</span>
          <span className="license-edition-name">{editionLabel(edition)}</span>
        </div>
        {license && (
          <Badge variant={BADGE_VARIANTS[statusTone(status)]}>{statusLabel(status)}</Badge>
        )}
        {storesLicense(response) && (
          <div className="license-summary-actions">
            <Button type="button" variant="ghost" size="sm" disabled={changing} onClick={onRemove}>
              Remove license
            </Button>
          </div>
        )}
      </div>
      {unverified && (
        <div className="alert alert-warning license-unverified" role="status">
          <p>
            A license key is stored, but this version of Zone can't verify it, so this server runs
            the free Community edition.
          </p>
          <p>{unverifiedReason(unverified.code)}</p>
        </div>
      )}
      {!storesLicense(response) && (
        <p className="license-community">
          No license is installed, so this server runs the free Community edition.
        </p>
      )}
      <dl className="license-details">
        {license && (
          <div className="license-detail">
            <dt>Customer</dt>
            <dd>{license.customer}</dd>
          </div>
        )}
        <div className="license-detail">
          <dt>Seats</dt>
          <dd>{seatText(seats)}</dd>
        </div>
        {license && (
          <>
            <div className="license-detail">
              <dt>{expiryLabel(status)}</dt>
              <dd>{licenseDate(license.expires_at)}</dd>
            </div>
            <div className="license-detail">
              <dt>Grace ends</dt>
              <dd>{licenseDate(license.grace_ends_at)}</dd>
            </div>
          </>
        )}
      </dl>
    </div>
  );
}

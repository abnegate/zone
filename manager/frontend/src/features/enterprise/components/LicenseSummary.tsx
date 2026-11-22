import { Button } from '@zone/ui';
import { memberText } from '../members';
import {
  editionLabel,
  expiryLabel,
  licenseDate,
  seatText,
  statusLabel,
  statusTone,
  storesLicense,
  unverifiedReason,
} from '../status';
import type { LicenseResponse } from '../types';
import { LicenseId } from './LicenseId';
import { LicenseRenewal } from './LicenseRenewal';
import { ToneBadge } from './ToneBadge';

interface LicenseSummaryProps {
  response: LicenseResponse;
  changing: boolean;
  onRemove: () => void;
}

export function LicenseSummary({ response, changing, onRemove }: LicenseSummaryProps) {
  const { edition, status, license, seats, members, unverified, renewal } = response;
  const membership = memberText(members);

  return (
    <div className="settings-card license-summary">
      <div className="license-summary-header">
        <div className="license-edition">
          <span className="license-edition-label">Edition</span>
          <span className="license-edition-name">{editionLabel(edition)}</span>
        </div>
        {license && <ToneBadge tone={statusTone(status)}>{statusLabel(status)}</ToneBadge>}
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
        {license && <LicenseId id={license.id} />}
        {license && (
          <div className="license-detail">
            <dt>Customer</dt>
            <dd>{license.customer}</dd>
          </div>
        )}
        {membership && (
          <div className="license-detail">
            <dt>Members</dt>
            <dd>{membership}</dd>
          </div>
        )}
        {(license || !membership) && (
          <div className="license-detail">
            <dt>Seats</dt>
            <dd>{seatText(seats)}</dd>
          </div>
        )}
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
        {renewal && <LicenseRenewal renewal={renewal} />}
      </dl>
    </div>
  );
}

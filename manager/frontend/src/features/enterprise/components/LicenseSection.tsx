import { Button, buttonVariants } from '@zone/ui';
import { useId, useState } from 'react';
import { useLicense, useLicenseChanging } from '../hooks';
import { needsLicense, PRICING_URL } from '../pricing';
import { LicenseStatus } from '../schemas';
import { overSeatNotice, statusNotice, storesLicense } from '../status';
import type { LicenseResponse } from '../types';
import { LicenseKeyForm } from './LicenseKeyForm';
import { LicenseSummary } from './LicenseSummary';
import { RemoveLicenseDialog } from './RemoveLicenseDialog';
import './LicenseSection.css';

export function LicenseSection() {
  const headingId = useId();
  const { response, loading, error, refresh } = useLicense();
  const changing = useLicenseChanging();
  const [removing, setRemoving] = useState<LicenseResponse | null>(null);

  if (loading) {
    return <div className="loading-state">Loading license...</div>;
  }

  const notice = response ? statusNotice(response) : null;
  const overSeats = response ? overSeatNotice(response.seats) : null;
  const lapsed = response?.status === LicenseStatus.Lapsed;

  return (
    <section className="license-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            License
          </h2>
          <p className="section-description">Verify a Zone Enterprise license on this server.</p>
        </div>
        {response && needsLicense(response.status) && (
          <div className="section-row-actions license-buy-actions">
            <a
              className={`license-buy ${buttonVariants({ variant: 'secondary', size: 'sm' })}`}
              href={PRICING_URL}
              target="_blank"
              rel="noopener noreferrer"
            >
              Buy a license
            </a>
          </div>
        )}
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {notice && (
        <div className={`alert ${lapsed ? 'alert-error' : 'alert-warning'}`} role="status">
          {notice}
        </div>
      )}
      {overSeats && (
        <div className="alert alert-warning" role="status">
          {overSeats}
        </div>
      )}
      {response && (
        <LicenseSummary
          response={response}
          changing={changing}
          onRemove={() => setRemoving(response)}
        />
      )}
      {response && <LicenseKeyForm licensed={storesLicense(response)} />}
      {removing && (
        <RemoveLicenseDialog
          verified={removing.unverified === null}
          onClose={() => setRemoving(null)}
        />
      )}
    </section>
  );
}

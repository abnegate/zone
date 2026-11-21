import {
  renewalAttemptDate,
  renewalModeLabel,
  renewalOutcomeLabel,
  renewalOutcomeTone,
  showsRenewalAttempt,
} from '../status';
import type { Renewal } from '../types';
import { ToneBadge } from './ToneBadge';

interface LicenseRenewalProps {
  renewal: Renewal;
}

export function LicenseRenewal({ renewal }: LicenseRenewalProps) {
  const { mode, outcome } = renewal;

  return (
    <>
      <div className="license-detail">
        <dt>Renewal</dt>
        <dd>{renewalModeLabel(mode)}</dd>
      </div>
      {showsRenewalAttempt(renewal) && (
        <div className="license-detail">
          <dt>Last renewal check</dt>
          <dd className="license-renewal-attempt">
            <span>{renewalAttemptDate(renewal)}</span>
            {outcome && (
              <>
                {' '}
                <ToneBadge tone={renewalOutcomeTone(outcome)}>
                  {renewalOutcomeLabel(outcome)}
                </ToneBadge>
              </>
            )}
          </dd>
        </div>
      )}
    </>
  );
}

import type { ReactNode } from 'react';
import type { Reconciliation } from '../types';
import { reconciliationFacts } from '../utils/reconciliation';

export const OPEN_RELEASES = 'Open releases on GitHub';

const RELEASES_PATH = '/releases';

export interface ReleaseWizardFailureProps {
  message: string;
  reconciliation: Reconciliation | null;
  repositoryUrl: string | null;
  action?: ReactNode;
}

export function ReleaseWizardFailure({
  message,
  reconciliation,
  repositoryUrl,
  action,
}: ReleaseWizardFailureProps) {
  return (
    <div className="error-banner release-wizard-failure" role="alert">
      <div className="release-wizard-failure-body">
        <span>{message}</span>
        {reconciliation && (
          <ul className="release-wizard-reconciliation">
            {reconciliationFacts(reconciliation).map((fact) => (
              <li key={fact}>{fact}</li>
            ))}
          </ul>
        )}
        {repositoryUrl && (
          <a href={`${repositoryUrl}${RELEASES_PATH}`} target="_blank" rel="noreferrer">
            {OPEN_RELEASES}
          </a>
        )}
      </div>
      {action}
    </div>
  );
}

import { Link } from 'react-router-dom';
import { LICENSE_PATH } from './PolicyUpgradeNotice';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Policies are still enforced; editing needs an active or grace license.';

export function PolicyLapsedNotice() {
  return (
    <div className="alert alert-warning policy-lapsed" role="status">
      <p>{LAPSED_NOTICE}</p>
      <Link to={LICENSE_PATH}>Renew on the License page</Link>
    </div>
  );
}

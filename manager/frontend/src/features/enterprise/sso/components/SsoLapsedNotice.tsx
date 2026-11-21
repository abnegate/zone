import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../licensePath';

export const LAPSED_NOTICE =
  'Your Zone license has lapsed. Sign-in with these providers and the single sign-on requirement keep working, but changing them needs a renewed license.';

export function SsoLapsedNotice() {
  return (
    <div className="alert alert-warning sso-lapsed" role="status">
      <p>{LAPSED_NOTICE}</p>
      <Link to={LICENSE_PATH}>Renew on the License page</Link>
    </div>
  );
}

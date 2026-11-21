import { Link } from 'react-router-dom';
import { LAPSED_NOTICE } from '../labels';
import { LICENSE_PATH } from './RolesUpgradeNotice';

export const RENEW_LINK = 'Renew on the License page';

export function RolesLapsedNotice() {
  return (
    <div className="alert alert-warning roles-lapsed" role="status">
      <p>{LAPSED_NOTICE}</p>
      <Link to={LICENSE_PATH}>{RENEW_LINK}</Link>
    </div>
  );
}

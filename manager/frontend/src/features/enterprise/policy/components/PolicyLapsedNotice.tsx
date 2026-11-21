import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../licensePath';
import { LAPSED_NOTICE } from '../labels';

export function PolicyLapsedNotice() {
  return (
    <div className="alert alert-warning policy-lapsed" role="status">
      <p>{LAPSED_NOTICE}</p>
      <Link to={LICENSE_PATH}>Renew on the License page</Link>
    </div>
  );
}

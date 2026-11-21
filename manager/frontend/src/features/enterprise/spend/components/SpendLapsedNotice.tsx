import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../../licensePath';
import { LAPSED_ACTION, LAPSED_NOTICE } from '../labels';

export function SpendLapsedNotice() {
  return (
    <div className="alert alert-warning spend-notice" role="status">
      <p>{LAPSED_NOTICE}</p>
      <Link to={LICENSE_PATH}>{LAPSED_ACTION}</Link>
    </div>
  );
}

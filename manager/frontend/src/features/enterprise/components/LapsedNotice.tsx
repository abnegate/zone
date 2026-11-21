import { Link } from 'react-router-dom';
import { LICENSE_PATH } from './UpgradeNotice';

export const RENEW_LINK = 'Renew on the License page';

interface LapsedNoticeProps {
  message: string;
  className: string;
}

export function LapsedNotice({ message, className }: LapsedNoticeProps) {
  return (
    <div className={`alert alert-warning ${className}`} role="status">
      <p>{message}</p>
      <Link to={LICENSE_PATH}>{RENEW_LINK}</Link>
    </div>
  );
}

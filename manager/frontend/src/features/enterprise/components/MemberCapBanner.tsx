import { buttonVariants } from '@zone/ui';
import { Link, useLocation } from 'react-router-dom';
import { useAuth } from '../../auth/context';
import { useLicense } from '../hooks/useLicense';
import { overMemberNotice } from '../members';
import { PRICING_URL } from '../pricing';
import { LICENSE_PATH } from './PlanUpgradeNotice';
import './MemberCapBanner.css';

export function MemberCapBanner() {
  const { user } = useAuth();
  const { pathname } = useLocation();
  const { response } = useLicense({ enabled: user?.is_admin === true });
  const notice = response ? overMemberNotice(response.members) : null;

  if (user?.is_admin !== true || notice === null || pathname === LICENSE_PATH) {
    return null;
  }

  return (
    <div className="member-cap-banner" role="status">
      <p className="member-cap-banner-text">{notice}</p>
      <div className="member-cap-banner-actions">
        <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
          Go to License
        </Link>
        <a
          className={buttonVariants({ variant: 'ghost', size: 'sm' })}
          href={PRICING_URL}
          target="_blank"
          rel="noopener noreferrer"
        >
          Compare plans
          <span className="sr-only"> (opens in a new tab)</span>
        </a>
      </div>
    </div>
  );
}

import { buttonVariants } from '@zone/ui';
import { Link } from 'react-router-dom';
import { LICENSE_PATH } from '../licensePath';
import { availableOn } from '../plans';
import type { Plan } from '../types';

interface PlanUpgradeNoticeProps {
  plan: Plan;
  message: string;
}

export function PlanUpgradeNotice({ plan, message }: PlanUpgradeNoticeProps) {
  return (
    <div className="settings-card plan-notice" role="note">
      <h3 className="plan-notice-title">{availableOn(plan)}</h3>
      <p>{message}</p>
      <Link to={LICENSE_PATH} className={buttonVariants({ variant: 'outline', size: 'sm' })}>
        Go to License
      </Link>
    </div>
  );
}

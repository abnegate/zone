import { buttonVariants } from '@zone/ui';
import { availableOn } from '../plans';
import type { Plan } from '../types';
import { LicenseLink } from './LicenseLink';

export const LICENSE_LINK = 'Go to License';

export const ASK_FOR_LICENSE = 'Ask your server admin to install a license.';

interface UpgradeNoticeProps {
  message: string;
  className: string;
  plan?: Plan;
  action?: string;
  request?: string;
  announced?: boolean;
}

export function UpgradeNotice({
  message,
  className,
  plan,
  action = LICENSE_LINK,
  request = ASK_FOR_LICENSE,
  announced = false,
}: UpgradeNoticeProps) {
  return (
    <div className={`settings-card ${className}`} role={announced ? 'alert' : 'note'}>
      {plan !== undefined && <h3 className="plan-notice-title">{availableOn(plan)}</h3>}
      <p>{message}</p>
      <LicenseLink
        label={action}
        request={request}
        className={buttonVariants({ variant: 'outline', size: 'sm' })}
      />
    </div>
  );
}
